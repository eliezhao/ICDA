mod batch_blob;
mod blob_id;
mod signature_management;
mod time_heap;

use crate::batch_blob::BatchCommit;
use crate::blob_id::BlobId;
use crate::time_heap::TimeHeap;
use candid::{candid_method, CandidType, Principal};
use ic_cdk::caller;
use ic_cdk_macros::{post_upgrade, pre_upgrade, query, update};
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap};
use serde::{Deserialize, Serialize};
use signature_management::SignatureQueue;
use std::cell::RefCell;

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

    static OWNER: RefCell<Principal> = RefCell::new(Principal::from_text("").unwrap());
}

// Retrieves the value associated with the given key if it exists.
// Return vec![] if key doesn't exit
#[candid_method(query)]
fn get_blob(key: String) -> Vec<u8> {
    MAP.with(|p| p.borrow().get(&key).unwrap_or_else(|| vec![]))
}

// Inserts an entry into the map and returns the previous value of the key if it exists.
// todo: call to signature，其他的都做好了
#[candid_method]
async fn save_blob(key: String, value: Vec<u8>) -> Result<(), String> {
    let blob_id: BlobId = serde_json::from_str(&key).unwrap();
    let mut flag = false;
    let mut commits = [BlobId::new(); 12]; // 这个用来给下面去call signature用的

    MAP.with(|p| {
        // 0. remove previous value from time heap and stable tree
        // 1. insert new value into time heap and tree
        TIMEHEAP.with(|t| {
            // insert new blob id into time heap and stable tree
            t.borrow_mut().insert(blob_id.clone());
            p.borrow_mut().insert(key, value);

            if let Some(previous_id) = t.borrow_mut().remove_expired() {
                let key = serde_json::to_string(&previous_id.0).unwrap();
                p.borrow_mut().remove(&key);
            }
        });

        // commit to batch
        BATCH.with(|b| {
            if let Some(com) = b.borrow_mut().insert(blob_id) {
                commits = com;
                flag = true;
            }
        });
    });

    // check if you should get signature
    // todo : 要清楚对什么东西进行sign
    if flag {
        match signature_management::sign("this is a message".to_string()).await {
            Ok(sig) => {
                SIGNATURES.with(|s| s.borrow_mut().insert(sig.signature_hex));
            }
            Err(e) => return Err(e),
        }
    }

    Ok(())
}

#[candid_method]
fn get_signature() -> Option<String> {
    SIGNATURES.with(|s| s.borrow_mut().pop())
}

#[candid_method]
fn change_owner(new_owner: Principal) {
    assert_eq!(caller(), OWNER.with(|o| o.borrow().clone()));
    OWNER.with(|o| *o.borrow_mut() = new_owner);
}

#[cfg(test)]
mod test {
    use crate::blob_id::BlobId;
    use crate::{get_blob, save_blob};

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
