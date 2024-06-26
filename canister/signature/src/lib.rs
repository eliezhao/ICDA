//! # Signature
//! confirmation
//! - signature on root
//! - merkle proof
//! - node digest
//!
//! 生成confirmation
//! - 1个batch生成1次signature
//! - 然后保存tree和root，signature
//! - 当需要获取confirmation的时候，就组成proof，然后生成confirmation
//! 保存confirmation
//! - 有1个key到了以后，可以获取到在哪个batch，所以需要key => batch index的map
//! - 有1个batch index => BatchConfirmation的map
//! 获取confirmation
//! - 通过key获取到batch index
//! - 通过batch index获取到BatchConfirmation结构体
//! - 通过tree和index生成proof，然后生成confirmation
//! 删除confirmation
//! - 每次生成1个confirmation，就说明可能有一个confirmation过期了,如果过期了就删除过期的confirmation

use std::cell::RefCell;

use candid::{candid_method, Principal};
use ic_cdk::{caller, print, spawn};
use ic_cdk_macros::{query, update};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap};
use rs_merkle::algorithms::Sha256;
use rs_merkle::MerkleTree;

use crate::confirmation::{BatchConfirmation, BatchInfo, Config, Confirmation};
use crate::signature::{
    mgmt_canister_id, ECDSAPublicKey, ECDSAPublicKeyReply, EcdsaKeyIds, PublicKeyReply,
    SignWithECDSA, SignWithECDSAReply, SignatureReply,
};

mod confirmation;
mod signature;

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    // confirmation config
    static CONFIRMATION_CONFIG: RefCell<Config> = RefCell::new(Config::default());

    // The memory manager is used for simulating multiple memories. Given a `MemoryId`
    // return a memory that can be used by stable structures.
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    // hex encode digest => batch index
    // "current_index" => current index
    static INDEX_MAP: RefCell<StableBTreeMap<String, BatchInfo, Memory>> = RefCell::new(StableBTreeMap::init(
        MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(0)))
    ));

    // batch index => BatchConfirmation
    static BATCH_CONFIRMATION: RefCell<StableBTreeMap<u32, BatchConfirmation, Memory>> = RefCell::new(StableBTreeMap::init(
        MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(0)))
    ));
}

// 获取confirmation
// - 通过key获取到batch index
// - 通过batch index获取到BatchConfirmation结构体
// - 通过tree和index生成proof，然后生成confirmation
#[query(name = "get_confirmation")]
#[candid_method]
fn get_confirmation(digest: [u8; 32]) -> Option<Confirmation> {
    let hex_digest = hex::encode(digest);
    let BatchInfo {
        batch_index,
        leaf_index,
    } = INDEX_MAP.with_borrow(|m| m.get(&hex_digest))?;
    let batch_confirmation = BATCH_CONFIRMATION.with_borrow(|m| m.get(&batch_index))?;

    let merkle_tree = MerkleTree::<Sha256>::from_leaves(&batch_confirmation.nodes);
    let root = merkle_tree.root()?;
    let proof = merkle_tree.proof(&[leaf_index]).proof_hashes().to_vec();

    let confirmation = Confirmation {
        root,
        proof,
        signature: batch_confirmation.signature.clone(),
    };

    Some(confirmation)
}

// 更新本地的digest
// digest: hex encoded digest
// 最后如果判断需要签名，就签名
// 删除过期的confirmation
#[update(name = "insert_digest")]
#[candid_method]
async fn insert_digest_and_generate_confirmation(digest: [u8; 32]) {
    assert!(check_updater(caller()), "only updater can insert digest");
    let hexed_digest = hex::encode(digest);

    INDEX_MAP.with_borrow_mut(|m| {
        // get current index
        let mut batch_info = m.get(&"current_index".to_string()).unwrap_or_default();

        // 更新current index
        // start with 1, leaf index: [1, BATCH_CONFIRMATION_SIZE]
        batch_info.leaf_index += 1;

        // 获取到的肯定是不满的current index，更新当前的batch index并且插入本key的batch index
        m.insert(hexed_digest, batch_info.clone());

        // insert into batch confirmation
        BATCH_CONFIRMATION.with_borrow_mut(|c| {
            let mut batch_confirmation = c.get(&batch_info.batch_index).unwrap_or_default();
            batch_confirmation.nodes.push(digest);
            c.insert(batch_info.batch_index, batch_confirmation);
        });

        // 判断是否已经满了，sign，并且更新current index
        if batch_info.leaf_index
            % CONFIRMATION_CONFIG.with_borrow(|c| c.confirmation_batch_size) as usize
            == 0
        {
            // prune maybe expired confirmation
            prune_expired_confirmation(batch_info.batch_index);

            // 更新current index
            let _batch_index = batch_info.batch_index;
            batch_info.batch_index = 1;
            batch_info.leaf_index = 0;
            m.insert("current_index".to_string(), batch_info);

            // sign
            spawn(update_signature(_batch_index));
        }
    });
}

#[update(name = "public_key")]
#[candid_method]
pub async fn public_key() -> Result<PublicKeyReply, String> {
    let request = ECDSAPublicKey {
        canister_id: None,
        derivation_path: vec![],
        key_id: EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (res,): (ECDSAPublicKeyReply,) =
        ic_cdk::call(mgmt_canister_id(), "ecdsa_public_key", (request,))
            .await
            .map_err(|e| format!("ecdsa_public_key failed {}", e.1))?;

    Ok(PublicKeyReply {
        public_key_hex: hex::encode(res.public_key),
    })
}

#[update(name = "update_config")]
fn update_confirmation_config(config: Config) {
    assert!(
        check_owner(caller()),
        "only owner can update signature batch size"
    );
    CONFIRMATION_CONFIG.with_borrow_mut(|c| *c = config);
}

// 1. update merkle root
// 2. sign merkle root([u8;32])
// 3. update signature
async fn update_signature(batch_index: u32) {
    // 获取batch confirmation
    let mut confirmation = BATCH_CONFIRMATION.with_borrow(|c| c.get(&batch_index).unwrap().clone());

    // 构建merkle tree
    let merkle_tree = MerkleTree::<Sha256>::from_leaves(&confirmation.nodes);

    // 获取 merkle root & 更新merkle root
    let root = merkle_tree.root().unwrap();
    confirmation.root = root;

    // sign merkle root
    match sign(root.to_vec()).await {
        Ok(SignatureReply { signature_hex }) => {
            confirmation.signature = signature_hex;
            // 更新batch confirmation & insert
            BATCH_CONFIRMATION.with_borrow_mut(|c| c.insert(batch_index, confirmation));
        }
        Err(e) => print(format!("sign failed: {}", e)),
    };
}

// sign [u8;32]
async fn sign(hash: Vec<u8>) -> Result<SignatureReply, String> {
    let request = SignWithECDSA {
        message_hash: hash,
        derivation_path: vec![],
        key_id: EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (response,): (SignWithECDSAReply,) = ic_cdk::api::call::call_with_payment(
        mgmt_canister_id(),
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

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service());
    assert_eq!(true, false)
}

fn prune_expired_confirmation(current_batch_index: u32) {
    if current_batch_index <= CONFIRMATION_CONFIG.with_borrow(|c| c.confirmation_live_time) {
        return;
    }

    let expired_batch_index =
        current_batch_index - CONFIRMATION_CONFIG.with_borrow(|c| c.confirmation_batch_size);
    BATCH_CONFIRMATION.with_borrow_mut(|c| {
        let expired_node_keys = c
            .get(&expired_batch_index)
            .unwrap()
            .nodes
            .iter()
            .map(hex::encode)
            .collect::<Vec<_>>();

        // remove batch confirmation
        c.remove(&expired_batch_index);

        // remove nodes index
        INDEX_MAP.with_borrow_mut(|m| {
            for key in expired_node_keys.iter() {
                m.remove(key);
            }
        });
    });
}

// 只有自己的canister才能写进来key
fn check_owner(c: Principal) -> bool {
    c.eq(&CONFIRMATION_CONFIG.with_borrow(|c| c.owner))
}

fn check_updater(c: Principal) -> bool {
    CONFIRMATION_CONFIG.with_borrow(|con| con.da_canisters.contains(&c))
}
