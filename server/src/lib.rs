use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};
use sha2::Digest;

pub mod upload;

pub mod disperser {
    #![allow(clippy::all)]
    tonic::include_proto!("disperser");
}

/// Blob identifier.
#[derive(Serialize, Deserialize, Debug)]
pub(crate) struct BlobId {
    /// Sha256 digest of the blob in hex format.
    pub(crate) digest: [u8; 32],

    /// Time since epoch in nanos.
    pub(crate) timestamp: u128,
}

impl BlobId {
    /// Creates the blob id for the blob.
    fn new(blob: &[u8]) -> Self {
        Self {
            digest: sha2::Sha256::digest(blob).into(),
            timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Failed to get timestamp")
                .as_nanos(),
        }
    }
}
