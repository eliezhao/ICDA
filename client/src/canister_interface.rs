use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::bail;
use anyhow::Result;
use candid::{Deserialize, Principal};
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::random;
use serde::Serialize;
use sha2::Digest;
use tracing::{error, info};

use crate::signature::{Confirmation, ConfirmationStatus, SignatureCanister};
use crate::storage::{BlobChunk, RoutingInfo, StorageCanister};

pub const REPLICA_NUM: usize = 2;

pub const COLLECTION_SIZE: usize = 20;

// 20 subnets with 40 canisters
pub const CANISTER_COLLECTIONS: [[&str; REPLICA_NUM]; COLLECTION_SIZE] =
    [["hxctj-oiaaa-aaaap-qhltq-cai", "v3y75-6iaaa-aaaak-qikaa-cai"]; COLLECTION_SIZE];

pub const SIGNATURE_CANISTER: &str = "r34pn-oaaaa-aaaak-qinga-cai";

// 1 week in nanos
pub const BLOB_LIVE_TIME: u128 = 7 * 24 * 60 * 60 * 1_000_000_000;
pub const CONFIRMATION_BATCH_SIZE: u32 = 12;
pub const CONFIRMATION_LIVE_TIME: u32 = 60 * 60 * 24 * 7 + 1; // 1 week in nanos

// canister存的时候主要用digest,time用server的time
#[derive(Serialize, Deserialize, Clone)]
pub struct BlobKey {
    pub digest: [u8; 32],
    pub expiry_timestamp: u128,
    pub routing_info: RoutingInfo,
}

impl Debug for BlobKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobKey")
            .field("digest", &hex::encode(&self.digest))
            .field("expiry_timestamp", &self.expiry_timestamp)
            .field("routing_info", &self.routing_info)
            .finish()
    }
}

#[derive(Clone)]
pub struct ICStorage {
    canister_collection_index: Option<u8>,
    storage_canisters_map: HashMap<Principal, StorageCanister>,
    signature_canister: SignatureCanister,
}

impl ICStorage {
    pub fn new(pem_path: &str) -> Result<Self> {
        let identity = BasicIdentity::from_pem_file(pem_path)?;
        let agent = Arc::new(
            Agent::builder()
                .with_url("https://ic0.app")
                .with_identity(identity)
                .build()
                .unwrap(),
        );

        let mut storage_canisters_map = HashMap::new();
        for storage_cid in CANISTER_COLLECTIONS.iter().flat_map(|x| x.iter()) {
            let sc =
                StorageCanister::new(Principal::from_text(storage_cid).unwrap(), agent.clone());
            storage_canisters_map.insert(Principal::from_text(storage_cid).unwrap(), sc);
        }

        let signature_canister = SignatureCanister::new(
            Principal::from_text(SIGNATURE_CANISTER).unwrap(),
            agent.clone(),
        );

        Ok(Self {
            canister_collection_index: None,
            storage_canisters_map,
            signature_canister,
        })
    }

    pub async fn save_blob(&mut self, blob: Vec<u8>) -> Result<Vec<u8>> {
        let blob_digest: [u8; 32] = sha2::Sha256::digest(&blob).into();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get timestamp")
            .as_nanos();

        let blob_chunks = Arc::new(BlobChunk::generate_chunks(&blob, blob_digest, timestamp));

        let storage_canisters = self.get_storage_canisters()?;
        let routing_canisters = storage_canisters
            .iter()
            .map(|sc| sc.canister_id)
            .collect::<Vec<_>>();

        let (tx, mut rx) = tokio::sync::mpsc::channel(REPLICA_NUM);
        for sc in storage_canisters {
            let _chunks = blob_chunks.clone();
            let _tx = tx.clone();
            tokio::spawn(async move {
                let cid = sc.canister_id;
                let res = Self::push_chunks_to_canister(sc, _chunks).await;
                let _ = _tx.send((cid, res)).await;
            });
        }

        let mut buffer = Vec::with_capacity(REPLICA_NUM);
        let _ = rx.recv_many(&mut buffer, REPLICA_NUM).await;
        rx.close();

        for (cid, res) in buffer.iter() {
            if let Err(e) = res {
                error!(
                    "ICStorage::save_blob_chunk(): cid = {}, error: {:?}",
                    cid.to_text(),
                    e
                );
            }
        }

        let blob_key = BlobKey {
            digest: blob_digest,
            expiry_timestamp: timestamp + BLOB_LIVE_TIME,
            routing_info: RoutingInfo {
                total_size: blob.len(),
                host_canisters: routing_canisters,
            },
        };

        let key = serde_json::to_string(&blob_key)?;
        Ok(key.as_bytes().to_vec())
    }

    pub async fn get_blob(&self, key: BlobKey) -> Result<Vec<u8>> {
        // inspect expiry timestamp
        let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        if key.expiry_timestamp < current_timestamp {
            bail!(
                "ICStorage::get_blob(): expired: key.expiry_timestamp = {:?}, current_timestamp = {:?}",
                key.expiry_timestamp,
                current_timestamp
            );
        }

        let storage_canisters = key
            .routing_info
            .host_canisters
            .iter()
            .map(|cid| {
                self.storage_canisters_map
                    .get(cid)
                    .expect("Failed to get storage canister")
                    .clone()
            })
            .collect::<Vec<_>>();

        // get from canisters
        let (tx, mut rx) = tokio::sync::mpsc::channel(REPLICA_NUM);
        for sc in storage_canisters {
            let _tx = tx.clone();
            let _key = key.clone();
            tokio::spawn(async move {
                let cid = sc.canister_id;
                let res = Self::get_blob_from_canister(sc, _key).await;
                let _ = _tx.send((cid, res)).await;
            });
        }
        tracing::info!("ICStorage::get_blob(): waiting for blobs");

        let mut res = Vec::with_capacity(REPLICA_NUM);
        let _ = rx.recv_many(&mut res, REPLICA_NUM).await;
        let blobs = res
            .iter()
            .filter_map(|(cid, res)| match res {
                Ok(blob) => Some(blob.clone()),
                Err(e) => {
                    error!(
                        "ICStorage::get_blob(): cid: {}, error: {:?}",
                        cid.to_text(),
                        e
                    );
                    None
                }
            })
            .collect::<Vec<_>>();

        for blob in blobs {
            let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
            if digest.eq(&key.digest) {
                info!("ICStorage::get_blob(): get blob successfully, digest match");
                return Ok(blob);
            } else {
                error!("ICStorage::get_blob(): blob digest not match\nkey digest:{:?}\nget blob digest{:?}", key.digest, digest);
            }
        }

        bail!("ICStorage::get_blob(): failed to get blob")
    }
}

impl ICStorage {
    // push chunks to a single canister
    async fn push_chunks_to_canister(
        sc: StorageCanister,
        chunks: Arc<Vec<BlobChunk>>,
    ) -> Result<()> {
        for (index, chunk) in chunks.iter().enumerate() {
            if let Err(e) = sc.save_blob(chunk).await {
                bail!(
                    "ICStorage::save_blob_chunk(): index = {index}, error: {:?}",
                    e
                );
            }
        }

        Ok(())
    }

    // get blob from a single canister
    async fn get_blob_from_canister(sc: StorageCanister, key: BlobKey) -> Result<Vec<u8>> {
        // 创建一样大小的buffer
        let mut blob = Vec::with_capacity(key.routing_info.total_size);

        let mut slice = sc.get_blob(key.digest).await?;
        blob.extend(slice.data);

        while let Some(next_index) = slice.next {
            // get blob by index
            slice = sc.get_blob_with_index(key.digest, next_index).await?;
            blob.extend(slice.data);
        }

        if blob.is_empty() {
            bail!("ICStorage::get_blob_from_canisters(): failed to get blob from canisters");
        }

        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        if !key.digest.eq(&digest) {
            bail!("ICStorage::get_blob_from_canisters(): blob digest not match");
        }

        Ok(blob)
    }

    pub async fn get_confirmation(
        sc: &SignatureCanister,
        digest: [u8; 32],
    ) -> Result<ConfirmationStatus> {
        match sc.get_confirmation(digest).await {
            Ok(confirmation) => Ok(confirmation),
            Err(e) => {
                bail!(
                    "ICStorage::get_confirmation(): failed to get confirmation, error: {}",
                    e
                );
            }
        }
    }

    pub async fn verify_confirmation(sc: &SignatureCanister, confirmation: &Confirmation) -> bool {
        sc.verify_confirmation(confirmation).await
    }

    // get storage canisters in the current round
    fn get_storage_canisters(&mut self) -> Result<Vec<StorageCanister>> {
        let index = self
            .canister_collection_index
            .get_or_insert(random::<u8>() % 20);

        let cids = CANISTER_COLLECTIONS.get(*index as usize).unwrap();

        *index += 1;
        *index %= 20;

        let storage_canisters = cids
            .iter()
            .map(|cid| {
                self.storage_canisters_map
                    .get(&Principal::from_text(cid).unwrap())
                    .expect("Failed to get storage canister")
                    .clone()
            })
            .collect::<Vec<_>>();

        Ok(storage_canisters)
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[tokio::test]
    async fn test_verify_confirmation() {
        let digest = [
            246, 78, 26, 27, 197, 96, 134, 30, 120, 14, 140, 108, 88, 191, 147, 150, 14, 59, 70,
            144, 20, 143, 31, 36, 83, 76, 179, 182, 222, 133, 179, 223,
        ];

        let ics = ICStorage::new("../bin/identity.pem").unwrap();

        let sc = ics.signature_canister.clone();

        // use sc get confirmation of the digest and verify
        let confirmation = sc.get_confirmation(digest).await.unwrap();

        match confirmation {
            ConfirmationStatus::Confirmed(confirmation) => {
                let res = sc.verify_confirmation(&confirmation).await;
                assert_eq!(res, true, "failed to verify confirmation");
            }
            ConfirmationStatus::Pending => {
                panic!("confirmation is pending")
            }
            ConfirmationStatus::Invalid => {
                panic!("digest is invalid")
            }
        }
    }
}
