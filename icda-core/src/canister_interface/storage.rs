use std::collections::HashSet;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;

use crate::canister_interface::rr_agent::RoundRobinAgent;
use crate::icda::{
    CANISTER_THRESHOLD, DEFAULT_OWNER, QUERY_RESPONSE_SIZE, SIGNATURE_CANISTER, TEST_IDENTITY,
};
use anyhow::bail;
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use serde::Serialize;

const CHUNK_SIZE: usize = 1 << 20; // 1 MB

#[derive(Deserialize, Serialize, CandidType, Debug, Clone)]
pub struct BlobChunk {
    /// index of the chunk
    pub index: usize,

    /// Sha256 digest of the blob in hex format.
    pub digest: [u8; 32], // hex encoded digest

    /// Time since epoch in nanos.
    pub timestamp: u128,

    /// blob总大小
    pub total: usize,

    /// chunk slice of total blob.
    pub data: Vec<u8>,
}

impl BlobChunk {
    pub fn generate_chunks(blob: Vec<u8>, digest: [u8; 32], timestamp: u128) -> Vec<BlobChunk> {
        // split to chunks
        let total = blob.len();
        let data_slice = Self::split_blob_into_chunks(blob);
        let mut chunks = Vec::with_capacity(data_slice.len());
        for (index, slice) in data_slice.iter().enumerate() {
            let chunk = BlobChunk {
                index,
                digest,
                timestamp,
                total,
                data: slice.to_owned(),
            };
            chunks.push(chunk);
        }
        chunks
    }

    fn split_blob_into_chunks(blob: Vec<u8>) -> Vec<Vec<u8>> {
        let mut chunks = Vec::new();
        let mut start = 0;

        while start < blob.len() {
            let end = (start + CHUNK_SIZE).min(blob.len());
            let chunk = blob[start..end].to_vec();
            chunks.push(chunk);
            start += CHUNK_SIZE;
        }

        chunks
    }
}

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct RoutingInfo {
    pub total_size: usize,
    pub host_canisters: Vec<Principal>,
}

impl Debug for RoutingInfo {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RoutingInfo")
            .field("total_size", &self.total_size)
            .field(
                "host_canisters",
                &self
                    .host_canisters
                    .iter()
                    .map(|p| p.to_text())
                    .collect::<Vec<_>>(),
            )
            .finish()
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
pub struct Blob {
    pub data: Vec<u8>,
    pub next: Option<u64>, // next start index
}

#[derive(Deserialize, Serialize, CandidType, Clone)]
pub struct StorageCanisterConfig {
    pub owner: HashSet<Principal>, // who can upload to da canister
    pub signature_canister: Principal,
    pub query_response_size: usize,
    pub canister_storage_threshold: u32,
}

impl Default for StorageCanisterConfig {
    fn default() -> Self {
        Self {
            owner: HashSet::from_iter(vec![
                Principal::from_text(DEFAULT_OWNER).unwrap(),
                Principal::from_text(TEST_IDENTITY).unwrap(),
            ]),
            signature_canister: Principal::from_text(SIGNATURE_CANISTER).unwrap(),
            query_response_size: QUERY_RESPONSE_SIZE,
            canister_storage_threshold: CANISTER_THRESHOLD,
        }
    }
}

#[derive(Clone)]
pub struct StorageCanister {
    pub agent: Arc<RoundRobinAgent>,
    pub canister_id: Principal,
}

impl StorageCanister {
    pub fn new(canister_id: Principal, agent: Arc<RoundRobinAgent>) -> Self {
        Self { agent, canister_id }
    }

    pub async fn get_blob(&self, digest: [u8; 32]) -> anyhow::Result<Blob> {
        let arg = Encode!(&digest)?;
        let raw_response = self
            .agent
            .query_call(&self.canister_id, "get_blob", arg)
            .await?;
        let response = Decode!(&raw_response, Blob)?;
        Ok(response)
    }

    pub async fn get_blob_with_index(&self, digest: [u8; 32], index: u64) -> anyhow::Result<Blob> {
        let arg = Encode!(&digest, &index)?;
        let raw_response = self
            .agent
            .query_call(&self.canister_id, "get_blob_with_index", arg)
            .await?;
        let response = Decode!(&raw_response, Blob)?;
        Ok(response)
    }

    pub async fn save_blob(&self, chunk: &BlobChunk) -> anyhow::Result<()> {
        let arg = Encode!(chunk)?;
        let raw_response = self
            .agent
            .update_call(&self.canister_id, "save_blob", arg)
            .await?;
        let response = Decode!(&raw_response, Result<(), String>)?;
        if let Err(e) = response {
            bail!("storage canister: save blob: failed to save blob: {}", e)
        }

        Ok(())
    }

    pub async fn notify_generate_confirmation(&self, digest: [u8; 32]) -> anyhow::Result<()> {
        let arg = Encode!(&digest)?;
        let _ = self
            .agent
            .update_call(&self.canister_id, "notify_generate_confirmation", arg)
            .await?;
        Ok(())
    }

    pub async fn update_config(&self, config: &StorageCanisterConfig) -> anyhow::Result<()> {
        let arg = Encode!(&config)?;
        let _ = self
            .agent
            .update_call(&self.canister_id, "update_config", arg)
            .await?;
        Ok(())
    }
}
