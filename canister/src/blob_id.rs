use candid::Deserialize;
use serde::Serialize;
use std::cmp::Ordering;

#[derive(Serialize, Deserialize, Debug, Clone, Copy)]
pub struct BlobId {
    /// Sha256 digest of the blob in hex format.
    pub digest: [u8; 32],

    /// Time since epoch in nanos.
    pub timestamp: u128,
}

impl BlobId {
    pub fn new() -> Self {
        let digest = [0; 32];
        let timestamp = 0;
        BlobId { digest, timestamp }
    }
}

impl Eq for BlobId {}

impl PartialEq<Self> for BlobId {
    fn eq(&self, other: &Self) -> bool {
        self.timestamp == other.timestamp && self.digest == other.digest
    }
}

impl PartialOrd<Self> for BlobId {
    fn partial_cmp(&self, other: &Self) -> Option<Ordering> {
        self.timestamp.partial_cmp(&other.timestamp)
    }
}

impl Ord for BlobId {
    fn cmp(&self, other: &Self) -> Ordering {
        self.timestamp.cmp(&other.timestamp)
    }
}
