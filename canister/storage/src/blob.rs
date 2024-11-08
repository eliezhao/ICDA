//! 上传处理三个事情
//! btree map
//! time heap
//! signature

use candid::{CandidType, Deserialize};
use ic_cdk::print;
use serde::Serialize;

use crate::{BLOBS, DACONFIG};

pub struct BlobData(pub Vec<u8>);

// upload 用
#[derive(Deserialize, Serialize, CandidType, Debug, Clone)]
pub struct BlobChunk {
    /// Segmented upload index.
    pub index: usize,

    /// Sha256 digest of the blob in hex format.
    pub digest: [u8; 32],

    /// Time since epoch in nanos.
    pub timestamp: u128,

    /// Total blob size in bytes.
    pub total: usize,

    /// chunk data: A piece of the blob.
    pub data: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
pub struct Blob {
    pub data: Vec<u8>,
    pub next: Option<usize>, // next index
}

// 1. 第一次上传，则创建一个空的vec，大小为total
// 2. 之后的上传，将chunk append到vec中
pub fn insert_to_store_map(
    hexed_digest: &String,
    index: usize,
    total_size: usize,
    data: &[u8],
) -> bool {
    BLOBS.with(|map| {
        let mut value = map
            .borrow()
            .get(hexed_digest)
            .unwrap_or_else(|| vec![0; total_size]);

        let chunk_size = DACONFIG.with_borrow(|c| c.chunk_size);

        let start = index * chunk_size;
        let end = (start + chunk_size).min(total_size);

        value[start..end].copy_from_slice(data);

        let _ = map.borrow_mut().insert(hexed_digest.to_string(), value);
        end == total_size
    })
}

pub fn remove_expired_blob_from_map(digest: [u8; 32]) {
    BLOBS.with(|map| {
        let hex_digest = hex::encode(digest);
        let v = map.borrow_mut().remove(&hex_digest);
        if v.is_some() {
            print(format!("remove expired blob of digest: {}", hex_digest));
        }
    })
}
