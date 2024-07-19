use rand::Rng;
use serde::{Deserialize, Serialize};
use sha2::Digest;
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncReadExt, AsyncWriteExt};

pub mod icda;
pub mod signature;
pub mod storage;

pub mod server;

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

pub const OWNER: &str = "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae";
pub const QUERY_RESPONSE_SIZE: usize = 2621440; // 2.5 * 1024 * 1024 = 2.5 MB
pub const CANISTER_THRESHOLD: u32 = 30240;
