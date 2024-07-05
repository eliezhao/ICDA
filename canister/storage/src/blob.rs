//! 上传处理三个事情
//! btree map
//! time heap
//! signature

use candid::{CandidType, Deserialize};
use ic_cdk::print;
use serde::Serialize;

use crate::BLOBS;

// upload 用
#[derive(Deserialize, Serialize, CandidType, Debug, Clone)]
pub struct BlobChunk {
    /// Sha256 digest of the blob in hex format.
    pub digest: [u8; 32],

    /// Time since epoch in nanos.
    pub timestamp: u128,

    /// blob总大小
    pub total: usize,

    /// The actual chunk.
    pub data: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
pub struct Blob {
    pub data: Vec<u8>,
    pub next: Option<usize>, // next start index
}

// 1. 第一次上传，则创建一个空的vec，大小为total
// 2. 之后的上传，将chunk append到vec中
pub fn insert_to_store_map(hexed_digest: String, total_size: usize, data: &Vec<u8>) {
    BLOBS.with(|map| {
        // 获取map中有无key - value
        if map.borrow().get(&hexed_digest).is_none() {
            // 没有，就insert，同时将vec的大小控制为总大小
            let value: Vec<u8> = Vec::with_capacity(total_size);
            map.borrow_mut().insert(hexed_digest.clone(), value);
        }

        let mut value = map.borrow().get(&hexed_digest).unwrap();
        value.extend_from_slice(&data);
        print(format!("save blob of digest: {}", hexed_digest));
        let _ = map.borrow_mut().insert(hexed_digest, value);
    })
}

pub fn remove_expired_blob_from_map(digest: [u8; 32]) {
    BLOBS.with(|map| {
        let hex_digest = hex::encode(digest);
        let _ = map.borrow_mut().remove(&hex_digest);
    })
}
