use std::cell::RefCell;

use candid::{candid_method, export_service, Principal};
use ic_cdk::caller;
use ic_cdk_macros::*;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableMinHeap};

use crate::blob::{Blob, BlobChunk, BlobId};
use crate::da::Config;
use crate::time_heap::handle_time_heap;

mod time_heap;

mod blob;
mod da;

type Memory = VirtualMemory<DefaultMemoryImpl>;

thread_local! {

    // da canister config
    static DACONFIG: RefCell<Config> = RefCell::new(Config::default()); // 2 round => 40s,1 round about 20s[20 subnet]

    // The memory manager is used for simulating multiple memories. Given a `MemoryId` it can
    // return a memory that can be used by stable structures.
    static MEMORY_MANAGER: RefCell<MemoryManager<DefaultMemoryImpl>> =
        RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));


    // Initialize a `StableBTreeMap` with `MemoryId(0)`.
    static BLOBS: RefCell<StableBTreeMap<String, Vec<u8>, Memory>> = RefCell::new(
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
fn get_blob(digest: [u8; 32]) -> Blob {
    let query_response_size = DACONFIG.with_borrow(|c| c.query_response_size);
    let key = hex::encode(digest);
    // vec![], None
    let mut blob = Blob::default();

    BLOBS.with_borrow(|m| {
        if let Some(data) = m.get(&key) {
            if data.len() > query_response_size {
                // 大于Query则分片，串行get
                blob.data.extend_from_slice(&data[..query_response_size]);
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
fn get_blob_with_index(digest: [u8; 32], index: usize) -> Blob {
    let query_response_size = DACONFIG.with_borrow(|c| c.query_response_size);

    let key = hex::encode(digest);

    let mut blob = Blob::default();

    BLOBS.with_borrow(|m| {
        if let Some(data) = m.get(&key) {
            if data.len() > query_response_size * (index + 1) {
                blob.data.extend_from_slice(
                    &data[query_response_size * index..query_response_size * (index + 1)],
                );
                blob.next = Some(index + 1);
            } else {
                blob.data
                    .extend_from_slice(&data[query_response_size * index..]);
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

    if blob_exist(chunk.digest) {
        BLOBS.with(|m| blob::handle_upload(m.borrow_mut(), &chunk));
    } else {
        // 1. insert new blob id into time heap
        // 2. remove expired blob id from time heap
        // 3. if expired blob id exists, return expired key
        let expired_key =
            TIMEHEAP.with(|t| handle_time_heap(t.borrow_mut(), chunk.digest, chunk.timestamp));

        // 1. insert blob share into map
        // 2. if expired blob id exists, remove it from a map
        BLOBS.with(|m| {
            // remove expired blob
            if let Some(expired_blob) = expired_key {
                let hex_digest = hex::encode(expired_blob.digest);
                m.borrow_mut().remove(&hex_digest);
            }

            blob::handle_upload(m.borrow_mut(), &chunk)
        });

        let _: Result<(), _> = ic_cdk::call(
            DACONFIG.with_borrow(|c| c.signature_canister),
            "insert_digest",
            (chunk.digest,),
        )
        .await;
    }

    Ok(())
}

#[update(name = "notify_generate_confirmation")]
#[candid_method]
async fn notify_generate_confirmation(digest: [u8; 32]) {
    let _: Result<(), _> = ic_cdk::call(
        DACONFIG.with_borrow(|c| c.signature_canister),
        "insert_digest",
        (digest,),
    )
    .await;
}

#[update(name = "update_config")]
#[candid_method]
fn update_config(config: Config) {
    assert!(check_caller(caller()), "only owner can change da config");

    DACONFIG.with_borrow_mut(|c| *c = config);
}

fn check_caller(c: Principal) -> bool {
    c.eq(&DACONFIG.with_borrow(|c| c.owner))
}

fn blob_exist(digest: [u8; 32]) -> bool {
    let hexed_digest = hex::encode(digest);
    BLOBS.with(|m| m.borrow().contains_key(&hexed_digest))
}

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service());
    assert_eq!(true, false)
}
