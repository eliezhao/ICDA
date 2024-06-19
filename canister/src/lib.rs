use std::cell::RefCell;

use candid::{candid_method, CandidType, Principal};
use ic_cdk::{caller, print};
use ic_cdk_macros::*;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableMinHeap};
use k256::pkcs8::der::Encode;
use rs_merkle::algorithms::Sha256;
use rs_merkle::MerkleTree;
use serde::{Deserialize, Serialize};

use crate::blob_id::BlobId;
use crate::signature_management::{PublicKeyReply, SignatureReply};
use crate::time_heap::handle_time_heap;
use crate::upload::BlobChunk;

mod blob_id;
mod signature_management;
mod time_heap;
mod upload;

type Memory = VirtualMemory<DefaultMemoryImpl>;

#[derive(CandidType, Deserialize, Serialize, Clone)]
struct Confirmation {
    pub digest: [u8; 32],     // merkle root hash
    pub proof: Vec<[u8; 32]>, // merkle proof
    pub signature: String,    // hex encoded signature
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
struct Blob {
    data: Vec<u8>,
    next: Option<usize>, // next start index
}

thread_local! {
    // 几轮sign一次，因为是rr，所以1=20个canister
    static SIGNATURE_BATCH_SIZE: RefCell<u32> = const { RefCell::new(2) }; // 2 round => 40s,1 round about 20s[20 subnet]

    // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
    // return a memory that can be used by stable structures.
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    // elie's local identity
    static OWNER: RefCell<Principal> = RefCell::new(Principal::from_text("ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae").unwrap());

    // Initialize a `StableBTreeMap` with `MemoryId(0)`.
    static MAP: RefCell<StableBTreeMap<String, Vec<u8>, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))),
        )
    );

    // time heap
    static TIMEHEAP: RefCell<StableMinHeap<BlobId ,Memory>> = RefCell::new(
        StableMinHeap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(1))),
        ).unwrap()
    );

    // canister signature: hex encoded secp256k1 signature
    static SIGNATURE: RefCell<String> = const { RefCell::new(String::new()) };

    // blob hash merkle tree
    static MERKLE_TREE: RefCell<MerkleTree<Sha256>> = RefCell::new(MerkleTree::new());

    // key: hexed hash, value: index in merkle tree(commited tree)
    static INDEX_MAP: RefCell<StableBTreeMap<String, u32, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(2))),
        )
    );
}

const QUERY_SIZE: usize = 2621440;

// Retrieves the value associated with the given key if it exists.
// Return vec![] if key doesn't exit
#[query(name = "get_blob")]
#[candid_method(query)]
fn get_blob(key: String) -> Blob {
    // vec![], None
    let mut blob = Blob::default();

    MAP.with_borrow(|m| {
        if let Some(data) = m.get(&key) {
            if data.len() > QUERY_SIZE {
                // 大于Query则分片，串行get
                blob.data.extend_from_slice(&data[..QUERY_SIZE]);
                blob.next = Some(1)
            } else {
                blob.data = data;
            }
        }
    });

    blob
}

#[query(name = "get_blob_with_index")]
#[candid_method(query)]
fn get_blob_with_index(key: String, index: usize) -> Blob {
    let mut blob = Blob::default();

    MAP.with_borrow(|m| {
        if let Some(data) = m.get(&key) {
            if data.len() > QUERY_SIZE * (index + 1) {
                blob.data
                    .extend_from_slice(&data[QUERY_SIZE * index..QUERY_SIZE * (index + 1)]);
                blob.next = Some(index + 1);
            } else {
                blob.data.extend_from_slice(&data[QUERY_SIZE * index..]);
            }
        }
    });

    blob
}

// Inserts an entry into the map and returns the previous value of the key if it exists.
#[update(name = "save_blob")]
#[candid_method]
async fn save_blob(chunk: BlobChunk) -> Result<(), String> {
    assert!(check_caller(caller()), "only owner can save blob");

    // 1. insert new blob id into time heap
    // 2. remove expired blob id from time heap
    // 3. if expired blob id exists, return expired key
    let expired_key =
        TIMEHEAP.with(|t| handle_time_heap(t.borrow_mut(), chunk.digest.clone(), chunk.timestamp));

    // 1. insert blob share into map
    // 2. if expired blob id exists, remove it from map
    MAP.with(|m| {
        // remove expired blob
        if let Some(expired_blob) = expired_key {
            m.borrow_mut().remove(&expired_blob.digest);
        }

        upload::handle_upload(m.borrow_mut(), &chunk)
    });

    // update merkle tree
    // if index % batch size == 0, then update merkle root and sign it
    if update_merkle_tree(chunk.digest) {
        // update signature
        update_signature().await;
    }

    Ok(())
}

#[update(name = "public_key")]
#[candid_method]
pub async fn public_key() -> Result<PublicKeyReply, String> {
    let request = signature_management::ECDSAPublicKey {
        canister_id: None,
        derivation_path: vec![],
        key_id: signature_management::EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (res,): (signature_management::ECDSAPublicKeyReply,) = ic_cdk::call(
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

#[update(name = "change_owner")]
#[candid_method]
fn change_owner(new_owner: Principal) {
    assert!(check_caller(caller()), "only owner can change owner");
    OWNER.with(|o| *o.borrow_mut() = new_owner);
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
    let hash: [u8; 32] = hex::decode(&digest).unwrap().try_into().unwrap();

    // update merkle tree
    MERKLE_TREE.with(|t| {
        let mut tree = t.borrow_mut();
        // insert blob's hash node
        tree.insert(hash);
    });

    let mut index = 0;

    // update index map
    // insert index到index map，这个index用来merkle tree做proof的时候用
    INDEX_MAP.with(|m| {
        let mut map = m.borrow_mut();
        // 获取index
        index = map.get(&"index".to_string()).unwrap_or_default();

        // index + 1
        map.insert("index".to_string(), index + 1);

        // insert hash and index
        map.insert(digest, index);
    });

    // 当index & batch size == 0的时候，就commit一次merkle root，然后返回true
    index % SIGNATURE_BATCH_SIZE.with(|s| *s.borrow()) == 0
}

fn check_caller(c: Principal) -> bool {
    OWNER.with(|o| o.borrow().eq(&c))
}

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service())
}

#[cfg(test)]
mod test {
    use rs_merkle::Hasher;

    use crate::upload::BlobChunk;

    use super::*;

    // test save blob and get blob, blob < QUERY_SIZE
    #[tokio::test]
    async fn test_save_blob_and_get_blob() {
        let data = vec![1, 2, 3, 4, 5];
        let digest = Sha256::hash(&data);
        let hex_encoded_digest = hex::encode(digest);

        let chunk = BlobChunk {
            digest: hex_encoded_digest.clone(),
            chunk: data,
            timestamp: 0,
            total: 5,
        };

        let res = super::save_blob(chunk).await;
        println!("save blob: {:?}", res);

        let blob = super::get_blob(hex_encoded_digest);
        assert_eq!(blob.next, None);
        assert_eq!(blob.data, vec![1, 2, 3, 4, 5]);
    }

    #[tokio::test]
    async fn test_upload_file_in_shares() {
        let first_slice = vec![1, 2, 3, 4, 5];
        let second_slice = vec![6, 7, 8, 9, 10];
        let digest = Sha256::hash(&vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
        let hex_encoded_digest = hex::encode(digest);

        let first_chunk = BlobChunk {
            digest: hex_encoded_digest.clone(),
            chunk: first_slice,
            timestamp: 0,
            total: 10,
        };

        let res = super::save_blob(first_chunk).await;
        assert_eq!(res, Ok(()));

        let blob = super::get_blob(hex_encoded_digest.clone());
        assert_eq!(blob.next, None);
        assert_eq!(blob.data, vec![1, 2, 3, 4, 5]);

        let second_chunk = BlobChunk {
            digest: hex_encoded_digest.clone(),
            chunk: second_slice,
            timestamp: 0,
            total: 10,
        };

        // push the second slice into the map
        let res = super::save_blob(second_chunk).await;
        assert_eq!(res, Ok(()));

        let blob = super::get_blob(hex_encoded_digest);
        assert_eq!(blob.next, None);
        assert_eq!(blob.data, vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 10]);
    }

    // test 大小大于3M的upload和get
    #[tokio::test]
    async fn test_upload_large_file() {
        let data = vec![1; 4 * 1024 * 1024]; // 4M
        let digest = Sha256::hash(&data);
        let hex_encoded_digest = hex::encode(digest);

        let upload_limit = 2 * 1024 * 1024 - 100; // 模仿upload

        let first_chunk = BlobChunk {
            digest: hex_encoded_digest.clone(),
            chunk: data[..upload_limit].to_vec(),
            timestamp: 0,
            total: 4 * 1024 * 1024,
        };

        let res = super::save_blob(first_chunk).await;
        assert_eq!(res, Ok(()));

        let blob = super::get_blob(hex_encoded_digest.clone());
        assert_eq!(blob.next, Some(1));
        assert_eq!(blob.data.len(), upload_limit);

        // insert the second slice
        let second_chunk = BlobChunk {
            digest: hex_encoded_digest.clone(),
            chunk: data[upload_limit..].to_vec(),
            timestamp: 0,
            total: 4 * 1024 * 1024,
        };

        let res = super::save_blob(second_chunk).await;
        assert_eq!(res, Ok(()));

        let blob = super::get_blob(hex_encoded_digest.clone());
        assert_eq!(blob.next, Some(1));
        assert_eq!(blob.data.len(), QUERY_SIZE);

        // get the second slice
        let blob = super::get_blob_with_index(hex_encoded_digest.clone(), 1);
        assert_eq!(blob.next, None);
        assert_eq!(blob.data.len(), 4 * 1024 * 1024 - QUERY_SIZE); // total - first_get_size(=QUERY_SIZE)
    }
}
