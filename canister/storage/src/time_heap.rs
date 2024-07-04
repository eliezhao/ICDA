use std::borrow::Cow;
use std::cmp::Ordering;

use candid::{CandidType, Decode, Deserialize, Encode};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::Storable;
use serde::Serialize;

use crate::config::CANISTER_THRESHOLD;
use crate::TIMEHEAP;

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

// 1. insert new blob id into time heap
// 2. remove expired blob id from time heap
// 3. if expired blob id exists, return expired key
pub fn insert_to_time_heap(digest: [u8; 32], timestamp: u128) -> Option<BlobId> {
    TIMEHEAP.with_borrow_mut(|heap| {
        let blob_id = BlobId { digest, timestamp };

        let _ = heap.push(&blob_id);

        // 删除过期的blob, 返回过期的blob
        if heap.len() > CANISTER_THRESHOLD as u64 {
            let expired_item = heap.pop();
            expired_item
        } else {
            None
        }
    })
}
