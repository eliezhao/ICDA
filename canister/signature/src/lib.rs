//! # Signature

use std::str::FromStr;

use candid::{candid_method, CandidType, Deserialize, Principal};
use ic_cdk::{caller, print};
use ic_cdk_macros::{query, update};
use serde::Serialize;

#[derive(CandidType, Deserialize, Serialize, Clone)]
struct Confirmation {
    pub digest: [u8; 32],     // merkle root hash
    pub proof: Vec<[u8; 32]>, // merkle proof
    pub signature: String,    // hex encoded signature
}

#[derive(CandidType, Serialize, Debug)]
pub struct PublicKeyReply {
    pub public_key_hex: String,
}

#[derive(CandidType, Serialize, Debug)]
pub struct SignatureReply {
    pub signature_hex: String,
}

type CanisterId = Principal;

#[derive(CandidType, Serialize, Debug)]
pub struct ECDSAPublicKey {
    pub canister_id: Option<CanisterId>,
    pub derivation_path: Vec<Vec<u8>>,
    pub key_id: EcdsaKeyId,
}

#[derive(CandidType, Deserialize, Debug)]
pub struct ECDSAPublicKeyReply {
    pub public_key: Vec<u8>,
    pub chain_code: Vec<u8>,
}

#[derive(CandidType, Serialize, Debug)]
pub struct SignWithECDSA {
    pub message_hash: Vec<u8>,
    pub derivation_path: Vec<Vec<u8>>,
    pub key_id: EcdsaKeyId,
}

#[derive(CandidType, Deserialize, Debug)]
pub struct SignWithECDSAReply {
    pub signature: Vec<u8>,
}

#[derive(CandidType, Serialize, Debug, Clone)]
pub struct EcdsaKeyId {
    pub curve: EcdsaCurve,
    pub name: String,
}

#[derive(CandidType, Serialize, Debug, Clone)]
pub enum EcdsaCurve {
    #[serde(rename = "secp256k1")]
    Secp256k1,
}

pub fn mgmt_canister_id() -> CanisterId {
    CanisterId::from_str("aaaaa-aa").unwrap()
}

#[derive(CandidType, Serialize, Debug, Clone)]
pub enum EcdsaKeyIds {
    #[allow(unused)]
    TestKeyLocalDevelopment,
    #[allow(unused)]
    TestKey1,
    #[allow(unused)]
    ProductionKey1,
}

impl EcdsaKeyIds {
    pub fn to_key_id(&self) -> EcdsaKeyId {
        EcdsaKeyId {
            curve: EcdsaCurve::Secp256k1,
            name: match self {
                Self::TestKeyLocalDevelopment => "dfx_test_key",
                Self::TestKey1 => "test_key_1",
                Self::ProductionKey1 => "key_1",
            }
            .to_string(),
        }
    }
}

getrandom::register_custom_getrandom!(always_fail);
pub fn always_fail(_buf: &mut [u8]) -> Result<(), getrandom::Error> {
    Err(getrandom::Error::UNSUPPORTED)
}

// thread_local! {
//     // 几轮sign一次，因为是rr，所以1=20个canister
//     static SIGNATURE_BATCH_SIZE: RefCell<u32> = const { RefCell::new(2) }; // 2 round => 40s,1 round about 20s[20 subnet]
//
//     // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
//     // return a memory that can be used by stable structures.
//     static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
//         RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));
//
//     // elie's local identity
//     static OWNER: RefCell<Principal> = RefCell::new(Principal::from_text("ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae").unwrap());
//
//     // time heap
//     static TIMEHEAP: RefCell<StableMinHeap<BlobId ,Memory>> = RefCell::new(
//         StableMinHeap::init(
//             MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))),
//         ).unwrap()
//     );
//
//     // canister signature: hex encoded secp256k1 signature
//     static SIGNATURE: RefCell<String> = const { RefCell::new(String::new()) };
//
//     // blob hash merkle tree
//     static MERKLE_TREE: RefCell<MerkleTree<Sha256>> = RefCell::new(MerkleTree::new());
//
//     // key: hexed hash, value: index in merkle tree(commited tree)
//     static INDEX_MAP: RefCell<StableBTreeMap<String, u32, Memory>> = RefCell::new(
//         StableBTreeMap::init(
//             MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2))),
//         )
//     );
// }

// 1. blob’s sha256 digest : [u8;32]
// 2. merkle proof
// 3. 对merkle root的signature
// key: blob sha256 digest
#[query(name = "get_confirmation")]
#[candid_method]
fn get_confirmation(key: String) -> Option<Confirmation> {
    let mut proof = Vec::new();
    let mut root = None;
    // get node index in merkle tree
    if let Some(node_index) = INDEX_MAP.with_borrow(|m| m.get(&key)) {
        MERKLE_TREE.with_borrow(|t| {
            // get proof
            proof = t.proof(&[node_index as usize]).proof_hashes().to_vec();

            // get root
            root = t.root();
        });
    };

    let signature = SIGNATURE.with_borrow(|s| s.clone());

    if root.is_some() {
        return Some(Confirmation {
            digest: hex::decode(key).unwrap().try_into().unwrap(),
            proof,
            signature,
        });
    }

    None
}

#[update(name = "speed_up_confirmation")]
#[candid_method]
async fn speed_up_confirmation() {
    assert!(
        check_caller(caller()),
        "only owner can speed up confirmation"
    );

    // update signature
    update_signature().await;
}

#[update(name = "update_signature_batch_size")]
fn update_signature_batch_size(size: u32) {
    assert!(
        check_caller(caller()),
        "only owner can update signature batch size"
    );
    SIGNATURE_BATCH_SIZE.with_borrow_mut(|s| *s = size);
}

#[update(name = "public_key")]
#[candid_method]
pub async fn public_key() -> Result<PublicKeyReply, String> {
    let request = ECDSAPublicKey {
        canister_id: None,
        derivation_path: vec![],
        key_id: signature_management::EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (res,): (ECDSAPublicKeyReply,) = ic_cdk::call(
        signature_management::mgmt_canister_id(),
        "ecdsa_public_key",
        (request,),
    )
    .await
    .map_err(|e| format!("ecdsa_public_key failed {}", e.1))?;

    Ok(PublicKeyReply {
        public_key_hex: hex::encode(res.public_key),
    })
}

// sign [u8;32]
async fn sign(hash: Vec<u8>) -> Result<SignatureReply, String> {
    let request = signature_management::SignWithECDSA {
        message_hash: hash,
        derivation_path: vec![],
        key_id: signature_management::EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (response,): (signature_management::SignWithECDSAReply,) =
        ic_cdk::api::call::call_with_payment(
            signature_management::mgmt_canister_id(),
            "sign_with_ecdsa",
            (request,),
            25_000_000_000,
        )
        .await
        .map_err(|e| format!("sign_with_ecdsa failed {}", e.1))?;

    Ok(SignatureReply {
        signature_hex: hex::encode(response.signature),
    })
}

// 1. update merkle root
// 2. sign merkle root([u8;32])
// 3. update signature
async fn update_signature() {
    // update merkle root
    if let Some(merkle_root) = MERKLE_TREE.with_borrow_mut(|t| {
        t.commit();
        t.root()
    }) {
        // sign merkle root
        match sign(merkle_root.to_vec().unwrap()).await {
            Ok(signature) => {
                // update signature
                SIGNATURE.with_borrow_mut(|s| {
                    *s = signature.signature_hex;
                })
            }
            Err(e) => {
                // print error log
                print(format!("Update canister signature failed, error: {}", e))
            }
        }
    }
}

// digest: blob's sha256 hash
fn update_merkle_tree(digest: String) -> bool {
    let mut index = 0;

    // update index map
    // insert index到index map，这个index用来merkle tree做proof的时候用
    INDEX_MAP.with_borrow_mut(|map| {
        if let Some(i) = map.get(&digest) {
            index = i;
        } else {
            let hash: [u8; 32] = hex::decode(&digest).unwrap().try_into().unwrap();

            // 获取index
            index = map.get(&"index".to_string()).unwrap_or_default();

            // index + 1
            map.insert("index".to_string(), index + 1);

            // insert hash and index
            map.insert(digest, index);

            // update merkle tree
            MERKLE_TREE.with_borrow_mut(|t| {
                // insert blob's hash node
                t.insert(hash);
            });
        }
    });

    //todo: 当该触发的时候，只用触发一次就行，
    // 当index & batch size == 0的时候，就commit一次merkle root，然后返回true
    index % SIGNATURE_BATCH_SIZE.with(|s| *s.borrow()) == 0
}

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service())
}
