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
use std::collections::HashSet;
use std::str::FromStr;

use candid::{candid_method, CandidType, Deserialize, Principal};
use ic_cdk::{caller, print};
use ic_cdk_macros::{query, update};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableMinHeap};
use rs_merkle::algorithms::Sha256;
use rs_merkle::MerkleTree;
use serde::Serialize;

use crate::types::{
    mgmt_canister_id, BatchConfirmation, BatchIndex, Confirmation, ECDSAPublicKey,
    ECDSAPublicKeyReply, EcdsaKeyIds, PublicKeyReply, SignWithECDSA, SignWithECDSAReply,
    SignatureReply,
};

mod types;

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    // 几轮sign一次，因为是rr，所以1=20个canister
    static CONFIRMATION_BATCH_SIZE: RefCell<u32> = const { RefCell::new(12) }; // 12 batch 1 confirmation

    // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
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
    let BatchIndex {
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

#[update(name = "update_confirmation_batch_size")]
fn update_confirmation_batch_size(size: u32) {
    assert_eq!(
        caller(),
        Principal::from_text("ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae")
            .unwrap(),
        "only owner can update signature batch size"
    );
    CONFIRMATION_BATCH_SIZE.with_borrow_mut(|s| *s = size);
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

// 生成confirmation
// - 1个batch生成1次signature
// - 然后保存tree和root，signature
// - 当需要获取confirmation的时候，就组成proof，然后生成confirmation
// 删除confirmation
// - 每次生成1个confirmation，就说明可能有一个confirmation过期了,如果过期了就删除过期的confirmation

// 保存confirmation
// - 有1个key到了以后，可以获取到在哪个batch，所以需要key => batch index的map
// - 有1个batch index => BatchConfirmation的map

// 更新本地的digest
// digest: hex encoded digest
// todo: 要写这个: 先获取current index,再更新index map，再更新batch confirmation，最后如果判断需要签名，就签名
#[update(name = "generate_confirmation")]
fn generate_confirmation(digest: [u8; 32]) {
    let raw_digest = hex::decode(&digest).unwrap();

    // insert to merkle tree
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

// 1. update merkle root
// 2. sign merkle root([u8;32])
// 3. update signature
async fn update_signature() {}

fn update_index_map() -> BatchIndex {
    // get batch index

    //

    BatchIndex::default()
}

// digest: blob's sha256 hash
fn update_merkle_tree(digest: &[u8; 32]) {
    // get hexed digest
    let hexed_digest = hex::encode(digest);

    // get current index & insert into index map

    // insert into batch confirmation

    // if batch_confirmation.size == batch size, then sign the signature
}

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service())
}

// 只有自己的canister才能写进来key
fn check_caller(c: Principal) -> bool {
    true
}

// 不知道这个干嘛用的
// getrandom::register_custom_getrandom!(always_fail);
// pub fn always_fail(_buf: &mut [u8]) -> Result<(), getrandom::Error> {
//     Err(getrandom::Error::UNSUPPORTED)
// }
