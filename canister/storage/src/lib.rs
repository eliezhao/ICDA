use std::cell::RefCell;
use std::ptr::hash;

use candid::{candid_method, CandidType, Principal};
use ic_cdk::{caller, print};
use ic_cdk_macros::*;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableMinHeap};
use serde::{Deserialize, Serialize};

use crate::blob_id::BlobId;
use crate::time_heap::handle_time_heap;
use crate::upload::BlobChunk;

mod blob_id;
mod time_heap;
mod upload;

type Memory = VirtualMemory<DefaultMemoryImpl>;

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
struct Blob {
    data: Vec<u8>,
    next: Option<usize>, // next start index
}

thread_local! {
    // todo : create signature canister
    static SIGNATURE_CANISTER: RefCell<Principal> = RefCell::new(Principal::from_text("").unwrap()); // 2 round => 40s,1 round about 20s[20 subnet]

    static QUERY_RESPONSE_SIZE: RefCell<usize> = const { RefCell::new(2621440) }; // 2.5 M

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
}

// Retrieves the value associated with the given key if it exists.
// Return vec![] if key doesn't exit
#[query(name = "get_blob")]
#[candid_method(query)]
fn get_blob(key: String) -> Blob {
    // get query size
    let query_size = QUERY_RESPONSE_SIZE.with(|q| *q.borrow());

    // vec![], None
    let mut blob = Blob::default();

    MAP.with_borrow(|m| {
        if let Some(data) = m.get(&key) {
            if data.len() > query_size {
                // 大于Query则分片，串行get
                blob.data.extend_from_slice(&data[..query_size]);
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
    let query_size = QUERY_RESPONSE_SIZE.with(|q| *q.borrow());
    let mut blob = Blob::default();

    MAP.with_borrow(|m| {
        if let Some(data) = m.get(&key) {
            if data.len() > query_size * (index + 1) {
                blob.data
                    .extend_from_slice(&data[query_size * index..query_size * (index + 1)]);
                blob.next = Some(index + 1);
            } else {
                blob.data.extend_from_slice(&data[query_size * index..]);
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
    // 2. if expired blob id exists, remove it from a map
    MAP.with(|m| {
        // remove expired blob
        if let Some(expired_blob) = expired_key {
            m.borrow_mut().remove(&expired_blob.digest);
        }

        upload::handle_upload(m.borrow_mut(), &chunk)
    });

    // todo: 参数统一String，内部做hex en/de code 处理
    // todo: api : push_key (digest: String) -> Result()
    let _: Result<(), _> = ic_cdk::call(
        SIGNATURE_CANISTER.with(|s| s.borrow().clone()),
        "push_key",
        (chunk.digest.clone(),),
    )
    .await;

    Ok(())
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

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service())
}