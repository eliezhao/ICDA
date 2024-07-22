use crate::canister_interface::signature::{ConfirmationStatus, SignatureCanister};
use crate::canister_interface::storage::{BlobChunk, RoutingInfo, StorageCanister};
use anyhow::bail;
use anyhow::Result;
use candid::{Deserialize, Principal};
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::random;
use serde::Serialize;
use sha2::Digest;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub const REPLICA_NUM: usize = 2;

pub const COLLECTION_SIZE: usize = 20;

// 20 subnets with 40 canisters
pub const CANISTER_COLLECTIONS: [[&str; REPLICA_NUM]; COLLECTION_SIZE] =
    [["hxctj-oiaaa-aaaap-qhltq-cai", "v3y75-6iaaa-aaaak-qikaa-cai"]; COLLECTION_SIZE];

pub const SIGNATURE_CANISTER: &str = "r34pn-oaaaa-aaaak-qinga-cai";

// 1 week in nanos
pub const BLOB_LIVE_TIME: u128 = 7 * 24 * 60 * 60 * 1_000_000_000;
pub const CONFIRMATION_BATCH_SIZE: u64 = 12;
pub const CONFIRMATION_LIVE_TIME: u32 = 60 * 60 * 24 * 7 + 1; // 1 week in nanos
pub const QUERY_RESPONSE_SIZE: usize = 2621440; // 2.5 * 1024 * 1024 = 2.5 MB
pub const CANISTER_THRESHOLD: u32 = 30240;

#[derive(Serialize, Deserialize, Clone)]
pub struct BlobKey {
    pub digest: [u8; 32],
    pub expiry_timestamp: u128, // current system time + live time
    pub routing_info: RoutingInfo,
}

impl Debug for BlobKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobKey")
            .field("digest", &hex::encode(self.digest))
            .field("expiry_timestamp", &self.expiry_timestamp)
            .field("routing_info", &self.routing_info)
            .finish()
    }
}

#[derive(Clone)]
pub struct ICDA {
    canister_collection_index: Arc<Mutex<u8>>,
    pub storage_canisters_map: HashMap<Principal, StorageCanister>,
    pub signature_canister: SignatureCanister,
}

impl ICDA {
    pub async fn new(pem_path: String) -> Result<Self> {
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

        match signature_canister.init().await {
            Ok(_) => {}
            Err(e) => bail!(
                "ICDA::new(): signature canister init failed, error: {:?}",
                e
            ),
        }

        let canister_collection_index = Arc::new(Mutex::new(random::<u8>() % 20));

        Ok(Self {
            canister_collection_index,
            storage_canisters_map,
            signature_canister,
        })
    }

    pub async fn push_blob_to_canisters(&self, blob: Vec<u8>) -> Result<BlobKey> {
        let blob_digest: [u8; 32] = sha2::Sha256::digest(&blob).into();

        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get timestamp")
            .as_nanos();

        let total_size = blob.len();

        let storage_canisters = self.get_storage_canisters().await;

        let routing_canisters = storage_canisters
            .iter()
            .map(|sc| sc.canister_id)
            .collect::<Vec<_>>();

        let (tx, mut rx) = tokio::sync::mpsc::channel(storage_canisters.len());

        let blob_chunks = Arc::new(BlobChunk::generate_chunks(blob, blob_digest, timestamp));

        let storage_canisters_num = storage_canisters.len();

        for sc in storage_canisters {
            let _chunks = blob_chunks.clone();
            let _tx = tx.clone();

            tokio::spawn(async move {
                let cid = sc.canister_id;
                let res = Self::push_chunks_to_canister(sc, _chunks).await;
                let _ = _tx.send((cid, res)).await;
                drop(_tx);
            });
        }

        for _ in 0..storage_canisters_num {
            if let Some((cid, Err(e))) = rx.recv().await {
                error!(
                    "ICDA::save_blob_chunk(): cid = {}, error: {:?}",
                    cid.to_text(),
                    e
                );
            }
        }

        rx.close();

        let blob_key = BlobKey {
            digest: blob_digest,
            expiry_timestamp: timestamp + BLOB_LIVE_TIME,
            routing_info: RoutingInfo {
                total_size,
                host_canisters: routing_canisters,
            },
        };

        Ok(blob_key)
    }

    pub async fn get_blob_from_canisters(&self, blob_key: BlobKey) -> Result<Vec<u8>> {
        let blob_key = Arc::new(blob_key);

        // inspect expiry timestamp
        let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        if blob_key.expiry_timestamp < current_timestamp {
            bail!(
                "ICDA::get_blob(): expired: key.expiry_timestamp = {:?}, current_timestamp = {:?}",
                blob_key.expiry_timestamp,
                current_timestamp
            );
        }

        let storage_canisters = blob_key
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
            let _key = blob_key.clone();
            let fut = async move {
                let cid = sc.canister_id;
                let res = Self::get_blob_from_canister(sc, _key).await;
                match res {
                    Ok(blob) => {
                        let _ = _tx.send(blob).await;
                    }
                    Err(e) => {
                        error!("ICDA::get_blob(): cid: {}, error: {:?}", cid.to_text(), e);
                    }
                }
                drop(_tx);
            };
            tokio::spawn(fut);
        }

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(blob) => {
                            return Ok(blob);
                        },
                        None => {
                            // No more senders and no message received
                            error!("All senders are closed and no more messages.");
                            break;
                        }
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if rx.is_closed() {
                        error!("ICDA: get_blob_from_canisters: all senders are closed");
                        break;
                    }
                }
            }
        }

        bail!("ICDA::get_blob(): failed to get blob")
    }

    pub async fn get_blob_confirmation(
        sc: &SignatureCanister,
        digest: [u8; 32],
    ) -> Result<ConfirmationStatus> {
        match sc.get_confirmation(digest).await {
            Ok(confirmation) => Ok(confirmation),
            Err(e) => {
                bail!(
                    "ICDA::get_confirmation(): failed to get confirmation, error: {}",
                    e
                );
            }
        }
    }
}

impl ICDA {
    // push chunks to a single canister
    async fn push_chunks_to_canister(
        sc: StorageCanister,
        chunks: Arc<Vec<BlobChunk>>,
    ) -> Result<()> {
        for chunk in chunks.iter() {
            // simple re-upload
            for i in 0..3 {
                if let Err(e) = sc.save_blob(chunk).await {
                    warn!(
                        "ICDA::save_blob_chunk(): cid: {}, error: {:?}, retry after 5 seconds",
                        sc.canister_id.to_text(),
                        e
                    );
                    if i == 2 {
                        bail!(
                            "ICDA::save_blob_chunk(): cid: {}, error: {:?}, retry 3 times failed",
                            sc.canister_id.to_text(),
                            e
                        );
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                }
            }
        }

        Ok(())
    }

    // get blob from canister and check digest
    async fn get_blob_from_canister(sc: StorageCanister, key: Arc<BlobKey>) -> Result<Vec<u8>> {
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
            bail!(
                "ICDA::get_blob_from_canisters(): failed to get blob from canisters, blob is empty"
            );
        }

        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        if !key.digest.eq(&digest) {
            bail!("ICDA::get_blob_from_canisters(): blob digest not match");
        }

        Ok(blob)
    }

    // get storage canisters in the current round
    async fn get_storage_canisters(&self) -> Vec<StorageCanister> {
        let cids;
        {
            let mut index = self.canister_collection_index.lock().await;
            cids = CANISTER_COLLECTIONS.get(*index as usize).unwrap();

            *index += 1;
            *index %= 20;
        }

        let storage_canisters = cids
            .iter()
            .map(|cid| {
                self.storage_canisters_map
                    .get(&Principal::from_text(cid).unwrap())
                    .expect("Failed to get storage canister")
                    .clone()
            })
            .collect::<Vec<_>>();

        storage_canisters
    }
}
