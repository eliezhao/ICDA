use std::cell::RefCell;

use candid::{candid_method, Principal};
use ic_cdk::{caller, print};
use ic_cdk_macros::*;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap};
use serde::{Deserialize, Serialize};

use signature_management::SignatureQueue;

use crate::batch_blob::BatchCommit;
use crate::blob_id::BlobId;
use crate::signature_management::{PublicKeyReply, SignatureReply};
use crate::time_heap::TimeHeap;

mod batch_blob;
mod blob_id;
mod signature_management;
mod time_heap;

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {
    // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
    // return a memory that can be used by stable structures.
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

    // Initialize a `StableBTreeMap` with `MemoryId(0)`.
    static MAP: RefCell<StableBTreeMap<String, Vec<u8>, Memory>> = RefCell::new(
        StableBTreeMap::init(
            MEMORY_MANAGER.with(|m| m.borrow().get(MemoryId::new(0))),
        )
    );

    // time heap
    static TIMEHEAP: RefCell<TimeHeap> = RefCell::new(TimeHeap::new());

    // commitments
    static BATCH: RefCell<BatchCommit> = RefCell::new(BatchCommit::new());

    // signature deque
    static SIGNATURES: RefCell<SignatureQueue> = RefCell::new(SignatureQueue::new());

    // elie's local identity
    static OWNER: RefCell<Principal> = RefCell::new(Principal::from_text("ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae").unwrap());
}

// Retrieves the value associated with the given key if it exists.
// Return vec![] if key doesn't exit
#[query(name = "get_blob")]
#[candid_method(query)]
fn get_blob(key: String) -> Vec<u8> {
    MAP.with(|p| p.borrow().get(&key).unwrap_or_default())
}

// Inserts an entry into the map and returns the previous value of the key if it exists.
#[update(name = "save_blob")]
#[candid_method]
async fn save_blob(key: String, value: Vec<u8>) -> Result<(), String> {
    assert!(check_caller(caller()), "only owner can save blob");
    let blob_id: BlobId = serde_json::from_str(&key).unwrap();

    let commits = MAP.with(|p| {
        // 0. remove previous value from time heap and stable tree
        // 1. insert new value into time heap and tree
        TIMEHEAP.with(|t| {
            // insert new blob id into time heap and stable tree
            t.borrow_mut().insert(blob_id);
            p.borrow_mut().insert(key, value);

            if let Some(previous_id) = t.borrow_mut().remove_expired() {
                let key = serde_json::to_string(&previous_id.0).unwrap();
                p.borrow_mut().remove(&key);
            }
        });

        // commit to batch and return commits
        BATCH.with(|b| b.borrow_mut().insert(blob_id))
    });

    // check if you should get signature
    // todo : 要清楚对什么东西进行sign, 用commits
    if let Some(_commits) = commits {
        let msg = "this is a message should be signed".to_string();
        match sign(msg.clone()).await {
            Ok(sig) => {
                print(format!("Signed Msg: {:?}", msg));
                SIGNATURES.with(|s| s.borrow_mut().insert(sig.signature_hex));
            }
            Err(e) => print(format!("Failed to sign msg:{}, Error Info: {:?}", msg, e)),
        }
    }

    Ok(())
}

// todo: 怎么get signature?
#[update(name = "get_signature")]
#[candid_method]
fn get_signature() -> Option<String> {
    assert!(check_caller(caller()), "only owner can get signature");
    SIGNATURES.with(|s| s.borrow_mut().pop())
}

// todo: production中获取一次public key然后保存就行
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

// todo: 暂时定为private，后续看情况修改
async fn sign(message: String) -> Result<SignatureReply, String> {
    let request = signature_management::SignWithECDSA {
        message_hash: signature_management::sha256(&message).to_vec(),
        derivation_path: vec![],
        key_id: signature_management::EcdsaKeyIds::ProductionKey1.to_key_id(),
    };

    let (response,): (signature_management::SignWithECDSAReply,) =
        ic_cdk::api::call::call_with_payment(
            signature_management::mgmt_canister_id(),
            "sign_with_ecdsa",
            (request,),
            25_000_000_000, // todo : cost?
        )
        .await
        .map_err(|e| format!("sign_with_ecdsa failed {}", e.1))?;

    Ok(SignatureReply {
        signature_hex: hex::encode(response.signature),
    })
}

#[update(name = "change_owner")]
#[candid_method]
fn change_owner(new_owner: Principal) {
    assert!(check_caller(caller()), "only owner can change owner");
    OWNER.with(|o| *o.borrow_mut() = new_owner);
}

fn check_caller(c: Principal) -> bool {
    OWNER.with(|o| o.borrow().eq(&c))
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test() {
        let blob_id_0 = BlobId {
            digest: [0; 32],
            timestamp: 0,
        };
        let key_0 = serde_json::to_string(&blob_id_0).unwrap();

        let blob_id_1 = BlobId {
            digest: [0; 32],
            timestamp: 1,
        };
        let key_1 = serde_json::to_string(&blob_id_1).unwrap();

        let save_0 = save_blob(key_0.clone(), vec![0]).await;
        assert_eq!(get_blob(key_0.clone()), vec![0]); // insert to tree
        let save_1 = save_blob(key_1.clone(), vec![1]).await;
        assert_eq!(get_blob(key_1), vec![1]); // insert to tree and heap
        assert_eq!(get_blob(key_0).len(), 0); // remove expired
    }
}
