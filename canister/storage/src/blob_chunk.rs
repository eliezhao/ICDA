/// 上传处理三个事情
/// btree map
/// time heap
/// signature
use std::cell::RefMut;

use candid::{CandidType, Deserialize};
use ic_stable_structures::BTreeMap;
use serde::Serialize;

use crate::Memory;

// upload 用
#[derive(Deserialize, Serialize, CandidType, Debug, Clone)]
pub struct BlobChunk {
    /// Sha256 digest of the blob in hex format.
    pub digest: [u8; 32], // hex encoded digest

    /// Time since epoch in nanos.
    pub timestamp: u128,

    /// blob总大小
    pub total: usize,

    /// The actual chunk.
    pub chunk: Vec<u8>,
}

// 1. 第一次上传，则创建一个空的vec，大小为total
// 2. 之后的上传，将chunk append到vec中
pub fn handle_upload(mut map: RefMut<BTreeMap<String, Vec<u8>, Memory>>, chunk: &BlobChunk) {
    // 获取map中有无key - value
    let hex_digest = hex::encode(&chunk.digest);
    if map.get(&hex_digest).is_none() {
        // 没有，就insert，同时将vec的大小控制为总大小
        let value: Vec<u8> = Vec::with_capacity(chunk.total);
        map.insert(hex_digest.clone(), value);
    }

    let mut value = map.get(&hex_digest).unwrap();
    value.extend_from_slice(&chunk.chunk);
    let _ = map.insert(hex_digest, value);
}
