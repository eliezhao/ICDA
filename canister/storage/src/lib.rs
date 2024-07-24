use crate::blob::{remove_expired_blob_from_map, Blob, BlobChunk};
use crate::config::Config;
use crate::time_heap::{insert_to_time_heap, BlobId};
use candid::{candid_method, Principal};
use ic_cdk::{caller, print, spawn};
use ic_cdk_macros::*;
use ic_stable_structures::memory_manager::{MemoryId, MemoryManager, VirtualMemory};
use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap, StableMinHeap};
use sha2::digest::core_api::{CoreWrapper, CtVariableCoreWrapper};
use sha2::digest::typenum::U32;
use sha2::{Digest, Sha256};
use sha2::{OidSha256, Sha256VarCore};
use std::cell::RefCell;
use std::collections::BTreeMap;

mod blob;
mod config;
mod time_heap;

type Memory = VirtualMemory<DefaultMemoryImpl>;
type Hasher = CoreWrapper<CtVariableCoreWrapper<Sha256VarCore, U32, OidSha256>>;

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

    static TEMP_DIGEST_STATE: RefCell<BTreeMap<String, Hasher>> = const { RefCell::new(BTreeMap::new()) };
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

    let hexed_digest = hex::encode(chunk.digest);

    // 1. insert into time heap
    //    新的blob到了，检查是否有expired，如果有就remove
    if !blob_exist(&hexed_digest) {
        // 1. if expired blob id exists, remove it from a map
        // remove expired blob
        let expired_key = insert_to_time_heap(chunk.digest, chunk.timestamp);
        if let Some(expired_blob) = expired_key {
            remove_expired_blob_from_map(expired_blob.digest)
        }
    }

    // 2. 更新digest state
    update_digest(&hexed_digest, chunk.index, &chunk.data);

    // 5. 如果match，再放入stable tree，并且spawn confirmation
    // 3. insert blob share into the map
    if blob::insert_to_store_map(&hexed_digest, chunk.index, chunk.total, &chunk.data) {
        if !check_digest(&hexed_digest, &chunk.digest) {
            print(format!("digest not match: {:?}", chunk.digest));
            // 6. 如果不match，从stable tree中删除
            BLOBS.with_borrow_mut(|m| {
                m.remove(&hexed_digest);
            });
            return Err(format!(
                "storage canister: digest not match: {}",
                hexed_digest
            ));
        } else {
            print(format!("saved blob, digest: {:?}", hexed_digest));
            // 3. notify signature canister to generate confirmation
            spawn(notify_generate_confirmation(chunk.digest));
        }
    };

    Ok(())
}

#[update(name = "notify_generate_confirmation")]
#[candid_method]
async fn notify_generate_confirmation(digest: [u8; 32]) {
    if !BLOBS.with_borrow(|b| b.contains_key(&hex::encode(digest))) {
        return;
    }

    match ic_cdk::call(
        DACONFIG.with_borrow(|c| c.signature_canister),
        "insert_digest",
        (digest,),
    )
    .await
    {
        Ok(()) => {}
        Err(e) => {
            print(format!("save_blob call signature_canister error: {:?}", e));
        }
    }
}

#[update(name = "update_config")]
#[candid_method]
fn update_config(config: Config) {
    assert!(check_caller(caller()), "only owner can change da config");

    DACONFIG.with_borrow_mut(|c| *c = config);
}

candid::export_service!();
#[test]
fn export_candid() {
    println!("{:#?}", __export_service());
}

fn check_caller(c: Principal) -> bool {
    c.eq(&DACONFIG.with_borrow(|c| c.owner))
}

fn blob_exist(hexed_digest: &String) -> bool {
    BLOBS.with(|m| m.borrow().contains_key(hexed_digest))
}

fn update_digest(key: &String, index: usize, slice: &[u8]) {
    // 检查是否存储过
    let chunk_size = DACONFIG.with_borrow(|c| c.chunk_size);
    if BLOBS.with_borrow(|m| m.get(key).unwrap_or_default().len()) > index * chunk_size {
        return;
    }

    TEMP_DIGEST_STATE.with_borrow_mut(|m| match m.get_mut(key) {
        Some(digest) => {
            digest.update(slice);
        }
        None => {
            let mut hasher = Sha256::new();
            hasher.update(slice);
            m.insert(key.clone(), hasher);
        }
    });
}

fn check_digest(key: &String, _digest: &[u8; 32]) -> bool {
    TEMP_DIGEST_STATE.with(|m| {
        if let Some(hasher) = m.borrow_mut().remove(key) {
            let result = hasher.finalize().to_vec();
            result.eq(_digest)
        } else {
            false
        }
    })
}
