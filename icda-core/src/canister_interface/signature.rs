use crate::canister_interface::rr_agent::RoundRobinAgent;
use crate::icda::{
    CANISTER_COLLECTIONS, COLLECTION_SIZE, CONFIRMATION_BATCH_SIZE, CONFIRMATION_LIVE_TIME,
    DEFAULT_OWNER,
};
use anyhow::{anyhow, Result};
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use rs_merkle::algorithms::Sha256;
use rs_merkle::MerkleProof;
use secp256k1::ecdsa::Signature;
use secp256k1::{Message, PublicKey, Secp256k1};
use serde::Serialize;
use std::collections::HashSet;
use std::sync::Arc;

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
            owner: Principal::from_text(DEFAULT_OWNER).unwrap(),
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
    pub agent: Arc<RoundRobinAgent>,
}

impl SignatureCanister {
    pub fn new(canister_id: Principal, agent: Arc<RoundRobinAgent>) -> Self {
        Self { canister_id, agent }
    }

    pub async fn update_config(&self, config: &SignatureCanisterConfig) -> Result<()> {
        let arg = Encode!(config).unwrap();
        let _ = self
            .agent
            .update_call(&self.canister_id, "update_config", arg)
            .await?;
        Ok(())
    }

    pub async fn public_key(&self) -> Result<Vec<u8>> {
        let raw = self
            .agent
            .query_call(&self.canister_id, "get_public_key", Encode!().unwrap())
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
            .update_call(&self.canister_id, "get_confirmation", arg)
            .await?;
        let confirmation = Decode!(&res, ConfirmationStatus)?;
        Ok(confirmation)
    }

    pub async fn verify_confirmation(&self, confirmation: &Confirmation) -> VerifyResult {
        let public_key = self.public_key().await.expect("failed to get public key");

        // verify signature
        let compact_sig =
            hex::decode(confirmation.signature.clone()).expect("failed to decode signature");
        let sig = Signature::from_compact(&compact_sig).expect("failed to parse signature");
        let msg = Message::from_digest_slice(confirmation.root.as_ref())
            .expect("failed to parse message");
        let pubkey = PublicKey::from_slice(&public_key).expect("failed to parse public key");
        let secp = Secp256k1::new();

        match secp.verify_ecdsa(&msg, &sig, &pubkey) {
            Ok(_) => {
                // verify merkle proof
                let merkle_proof =
                    MerkleProof::<Sha256>::try_from(confirmation.proof.proof_bytes.as_slice())
                        .expect("failed to parse merkle proof");

                if merkle_proof.verify(
                    confirmation.root,
                    &[confirmation.proof.leaf_index],
                    &[confirmation.proof.leaf_digest],
                    6,
                ) {
                    VerifyResult::Valid
                } else {
                    VerifyResult::InvalidProof
                }
            }
            Err(e) => VerifyResult::InvalidSignature(e.to_string()),
        }
    }

    pub async fn init(&self) -> Result<()> {
        let _ = self
            .agent
            .update_call(&self.canister_id, "init", Encode!().unwrap())
            .await?;
        Ok(())
    }
}
