//! 上传处理三个事情
//! btree map
//! time heap
//! signature

use std::borrow::Cow;
use std::cell::RefMut;
use std::cmp::Ordering;

use candid::{CandidType, Decode, Deserialize, Encode};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::{BTreeMap, Storable};
use serde::Serialize;

use crate::time_heap::CANISTER_THRESHOLD;
use crate::Memory;

// 1. 第一次上传，则创建一个空的vec，大小为total
// 2. 之后的上传，将chunk append到vec中
pub fn insert_map(mut map: RefMut<BTreeMap<String, Vec<u8>, Memory>>, chunk: &BlobChunk) {
    // 获取map中有无key - value
    let hex_digest = hex::encode(chunk.digest);
    if map.get(&hex_digest).is_none() {
        // 没有，就insert，同时将vec的大小控制为总大小
        let value: Vec<u8> = Vec::with_capacity(chunk.total);
        map.insert(hex_digest.clone(), value);
    }

    let mut value = map.get(&hex_digest).unwrap();
    value.extend_from_slice(&chunk.data);
    let _ = map.insert(hex_digest, value);
}

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
    pub data: Vec<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
pub struct Blob {
    pub data: Vec<u8>,
    pub next: Option<usize>, // next start index
}

#[derive(CandidType, Serialize, Deserialize, Debug, Clone)]
pub struct BlobId {
    /// Sha256 digest of the blob in hex format.
    pub digest: [u8; 32], // hex encoded digest

    /// Time since epoch in nanos.
    pub timestamp: u128,
}

impl Eq for BlobId {}

impl PartialEq<Self> for BlobId {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp && self.digest == other.digest
    }
}

impl PartialOrd<Self> for BlobId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for BlobId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}

impl Storable for BlobId {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: CANISTER_THRESHOLD,
        is_fixed_size: false,
    };
}
