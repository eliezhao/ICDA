use std::collections::HashSet;
use std::sync::Arc;

use anyhow::{anyhow, Result};
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_agent::Agent;
use rs_merkle::algorithms::Sha256;
use rs_merkle::MerkleProof;
use secp256k1::ecdsa::Signature;
use secp256k1::{Message, PublicKey, Secp256k1};
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
pub struct Config {
    pub confirmation_batch_size: u32,
    pub confirmation_live_time: u32,
    pub da_canisters: HashSet<Principal>,
    pub owner: Principal, // who can change confirmation config
}

#[derive(Clone)]
pub struct SignatureCanister {
    canister_id: Principal,
    agent: Arc<Agent>,
}

impl SignatureCanister {
    pub fn new(canister_id: Principal, agent: Arc<Agent>) -> Self {
        Self { canister_id, agent }
    }

    pub async fn update_config(&self, config: &Config) -> Result<()> {
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
            .query(&self.canister_id, "public_key")
            .with_arg(Encode!().unwrap())
            .call()
            .await?;
        let res = Decode!(&raw, core::result::Result<Vec<u8>, String>)?;

        if let Err(e) = res {
            return Err(anyhow!(e));
        }

        Ok(res.unwrap())
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

    pub async fn verify_confirmation(&self, confirmation: &Confirmation) -> bool {
        let public_key = self.public_key().await.expect("failed to get public key");

        // verify signature
        let compact_sig =
            hex::decode(confirmation.signature.clone()).expect("failed to decode signature");
        let sig = Signature::from_compact(&compact_sig).expect("failed to parse signature");
        let msg = Message::from_digest_slice(confirmation.root.as_ref())
            .expect("failed to parse message");
        let pubkey = PublicKey::from_slice(&public_key).expect("failed to parse public key");
        let secp = Secp256k1::new();
        if !secp.verify_ecdsa(&msg, &sig, &pubkey).is_ok() {
            return false;
        }

        // verify merkle proof
        let merkle_proof =
            MerkleProof::<Sha256>::try_from(confirmation.proof.proof_bytes.as_slice())
                .expect("failed to parse merkle proof");

        merkle_proof.verify(
            confirmation.root,
            &[confirmation.proof.leaf_index],
            &[confirmation.proof.leaf_digest],
            12,
        )
    }
}
