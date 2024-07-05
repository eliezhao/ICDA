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

use crate::confirmation::{
    BatchConfirmation, BatchIndex, Config, Confirmation, ConfirmationStatus, Proof,
};
use crate::signature::{
    mgmt_canister_id, ECDSAPublicKey, ECDSAPublicKeyReply, EcdsaKeyIds, SignWithECDSA,
    SignWithECDSAReply, SignatureReply,
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
    static INDEX_MAP: RefCell<StableBTreeMap<String, BatchIndex, Memory>> = RefCell::new(StableBTreeMap::init(
        MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(0)))
    ));

    // batch index => BatchConfirmation
    static BATCH_CONFIRMATION: RefCell<StableBTreeMap<u32, BatchConfirmation, Memory>> = RefCell::new(StableBTreeMap::init(
        MEMORY_MANAGER.with_borrow(|m| m.get(MemoryId::new(1)))
    ));

    static PUBLIC_KEY: RefCell<Vec<u8>> = RefCell::new(Vec::new());
}

const CURRENT_INDEX_KEY: &str = "current_index";

// 获取confirmation
// - 通过key获取到batch index
// - 通过batch index获取到BatchConfirmation结构体
// - 通过tree和index生成proof，然后生成confirmation
#[query(name = "get_confirmation")]
#[candid_method]
fn get_confirmation(digest: [u8; 32]) -> ConfirmationStatus {
    let hex_digest = hex::encode(digest);
    match INDEX_MAP.with_borrow(|m| m.get(&hex_digest)) {
        None => ConfirmationStatus::Invalid,
        Some(BatchIndex(batch_index)) => {
            let batch_confirmation = BATCH_CONFIRMATION
                .with_borrow(|m| m.get(&batch_index))
                .unwrap();
            if batch_confirmation.signature.is_none() {
                return ConfirmationStatus::Pending;
            }

            let merkle_tree = MerkleTree::<Sha256>::from_leaves(&batch_confirmation.nodes);

            let leaf_index = match batch_confirmation.nodes.iter().position(|&x| x == digest) {
                None => return ConfirmationStatus::Invalid,
                Some(index) => index,
            };

            let root = batch_confirmation.root;
            let proof_bytes = merkle_tree.proof(&[leaf_index]).to_bytes();

            let proof = Proof {
                proof_bytes,
                leaf_index,
                leaf_digest: digest,
            };

            let confirmation = Confirmation {
                root,
                proof,
                signature: batch_confirmation.signature.unwrap(),
            };

            ConfirmationStatus::Confirmed(confirmation)
        }
    }
}

// 更新本地的digest
// digest: hex encoded digest
// 最后如果判断需要签名，就签名
// 删除过期的confirmation
#[update(name = "insert_digest")]
#[candid_method]
async fn insert_digest(digest: [u8; 32]) {
    assert!(check_updater(caller()), "only updater can insert digest");
    let digest_hex = hex::encode(digest);

    let mut confirmation_update_info = None;

    INDEX_MAP.with(|index_map| {
        if index_map.borrow().contains_key(&digest_hex) {
            return;
        }

        let current_index = index_map
            .borrow()
            .get(&CURRENT_INDEX_KEY.to_string())
            .unwrap_or_default()
            .0;

        index_map
            .borrow_mut()
            .insert(digest_hex.clone(), BatchIndex(current_index));

        BATCH_CONFIRMATION.with(|batch_map| {
            let mut batch_confirmation = batch_map.borrow().get(&current_index).unwrap_or_default();
            batch_confirmation.nodes.push(digest);
            batch_map
                .borrow_mut()
                .insert(current_index, batch_confirmation.clone());

            if batch_confirmation.nodes.len()
                % CONFIRMATION_CONFIG.with_borrow(|config| config.confirmation_batch_size)
                == 0
            {
                prune_expired_confirmation(current_index);

                let new_current_index = current_index + 1;
                index_map
                    .borrow_mut()
                    .insert(CURRENT_INDEX_KEY.to_string(), BatchIndex(new_current_index));

                confirmation_update_info = Some((current_index, batch_confirmation));
            }
        });
    });

    if let Some((batch_index, confirmation)) = confirmation_update_info {
        spawn(update_signature(batch_index, confirmation));
    }
}

#[update(name = "public_key")]
#[candid_method]
pub async fn public_key() -> Result<Vec<u8>, String> {
    let request = ECDSAPublicKey {
        canister_id: None,
        derivation_path: vec![],
        key_id: EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (res,): (ECDSAPublicKeyReply,) =
        ic_cdk::call(mgmt_canister_id(), "ecdsa_public_key", (request,))
            .await
            .map_err(|e| format!("ecdsa_public_key failed {}", e.1))?;

    Ok(res.public_key)
}

#[update(name = "update_config")]
#[candid_method]
fn update_config(config: Config) {
    assert!(
        check_owner(caller()),
        "only owner can update signature batch size"
    );
    CONFIRMATION_CONFIG.with_borrow_mut(|c| *c = config);
}

#[update(name = "init")]
#[candid_method]
fn init() {
    assert!(
        check_owner(caller()),
        "only owner can update signature batch size"
    );

    // init public key
}

// 1. update merkle root
// 2. sign merkle root([u8;32])
// 3. update signature
async fn update_signature(batch_index: u32, batch_confirmation: BatchConfirmation) {
    // 获取batch confirmation
    let mut confirmation = batch_confirmation;

    // 构建merkle tree
    let merkle_tree = MerkleTree::<Sha256>::from_leaves(&confirmation.nodes);

    // 获取 merkle root & 更新merkle root
    let root = merkle_tree.root().unwrap();
    confirmation.root = root;

    // sign merkle root
    match sign(root.to_vec()).await {
        Ok(SignatureReply { signature_hex }) => {
            confirmation.signature = Some(signature_hex);
            // 更新batch confirmation & insert
            print("signed confirmation");
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
        // more than 26_153_846_153,
        // which specified in :https://internetcomputer.org/docs/current/references/t-ecdsa-how-it-works/#api
        27_000_000_000,
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
}

fn prune_expired_confirmation(current_batch_index: u32) {
    let confirmation_live_time = CONFIRMATION_CONFIG.with_borrow(|c| c.confirmation_live_time);

    if current_batch_index <= confirmation_live_time {
        return;
    }

    let expired_batch_index = current_batch_index - confirmation_live_time;

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
