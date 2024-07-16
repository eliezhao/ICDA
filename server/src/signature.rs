use std::collections::HashSet;
use std::sync::Arc;

use crate::ic_storage::{
    CANISTER_COLLECTIONS, COLLECTION_SIZE, CONFIRMATION_BATCH_SIZE, CONFIRMATION_LIVE_TIME,
};
use anyhow::{anyhow, Result};
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_agent::Agent;
use serde::Serialize;

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Proof {
    pub proof_bytes: Vec<u8>,
    pub leaf_index: usize,
    pub leaf_digest: [u8; 32],
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct Confirmation {
    pub root: [u8; 32],    // merkle root hash
    pub proof: Proof,      // merkle proof
    pub signature: String, // hex encoded signature
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub enum ConfirmationStatus {
    Pending,
    Confirmed(Confirmation),
    Invalid,
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct SignatureCanisterConfig {
    pub confirmation_batch_size: u64,
    pub confirmation_live_time: u32,
    pub da_canisters: HashSet<Principal>,
    pub owner: Principal, // who can change confirmation config
}

impl Default for SignatureCanisterConfig {
    fn default() -> Self {
        let mut da_canisters = HashSet::with_capacity(COLLECTION_SIZE);
        CANISTER_COLLECTIONS.iter().for_each(|x| {
            x.iter().for_each(|x| {
                da_canisters.insert(Principal::from_text(x).unwrap());
            });
        });

        Self {
            confirmation_live_time: CONFIRMATION_LIVE_TIME, // 7 days in batch number
            confirmation_batch_size: CONFIRMATION_BATCH_SIZE, // 12 blobs per confirmation
            da_canisters,
            owner: Principal::from_text(
                "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae",
            )
            .unwrap(),
        }
    }
}

pub enum VerifyResult {
    InvalidSignature(String),
    InvalidProof,
    Valid,
}

#[derive(Clone)]
pub struct SignatureCanister {
    pub canister_id: Principal,
    pub agent: Arc<Agent>,
}

impl SignatureCanister {
    pub fn new(canister_id: Principal, agent: Arc<Agent>) -> Self {
        Self { canister_id, agent }
    }

    pub async fn update_config(&self, config: &SignatureCanisterConfig) -> Result<()> {
        let arg = Encode!(config).unwrap();
        let _ = self
            .agent
            .update(&self.canister_id, "update_config")
            .with_arg(arg)
            .call_and_wait()
            .await?;
        Ok(())
    }

    pub async fn public_key(&self) -> Result<Vec<u8>> {
        let raw = self
            .agent
            .query(&self.canister_id, "get_public_key")
            .with_arg(Encode!().unwrap())
            .call()
            .await?;
        let res = Decode!(&raw, Vec<u8>)?;

        if res.is_empty() {
            return Err(anyhow!("public key is not init"));
        }

        Ok(res)
    }

    pub async fn get_confirmation(&self, digest: [u8; 32]) -> Result<ConfirmationStatus> {
        let arg = Encode!(&digest)?;
        let res = self
            .agent
            .update(&self.canister_id, "get_confirmation")
            .with_arg(arg)
            .call_and_wait()
            .await?;
        let confirmation = Decode!(&res, ConfirmationStatus)?;
        Ok(confirmation)
    }

    pub async fn init(&self) -> Result<()> {
        let _ = self
            .agent
            .update(&self.canister_id, "init")
            .with_arg(Encode!().unwrap())
            .call_and_wait()
            .await?;
        Ok(())
    }
}
