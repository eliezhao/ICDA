use std::ptr::hash;
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Error};
use backon::{BlockingRetryable, ExponentialBuilder, Retryable};
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use futures::future::{err, join_all};
use futures::StreamExt;
use ic_agent::agent::Transport;
use ic_agent::hash_tree::label;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::random;
use serde::Serialize;
use sha2::Digest;
use tracing::{error, info, warn};

// 20 subnets with 40 canisters
const CANISTER_COLLECTIONS: [[&str; 2]; 20] =
    [["hxctj-oiaaa-aaaap-qhltq-cai", "v3y75-6iaaa-aaaak-qikaa-cai"]; 20]; // 测试用 canisters

const BASIC_TIMESTAMP: u128 = 0; // start time stamp

const CHUNK_SIZE: usize = 1 << 20; // 1 MB

const LIVE_TIME: u128 = 60 * 60 * 24 * 7; // 1 week

#[derive(Serialize, Deserialize, Debug)]
pub struct BlobId {
    /// Sha256 digest of the blob in hex format.
    pub(crate) digest: [u8; 32],

    /// Time since epoch in nanos.
    pub(crate) timestamp: u128,
}

impl BlobId {
    /// Creates the blob Id for the blob.
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

#[derive(Deserialize, Serialize, CandidType, Debug)]
struct BlobChunk {
    /// Sha256 digest of the blob in hex format.
    digest: [u8; 32],

    /// Time since epoch in nanos.
    timestamp: u128,

    /// Index of the chunk.
    index: usize,

    /// Total number of chunks.
    total: usize,

    /// The actual chunk.
    chunk: Vec<u8>,
}

impl BlobChunk {
    pub fn generate_chunks(blob: &[u8]) -> Vec<BlobChunk> {
        let digest = sha2::Sha256::digest(blob).into();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get timestamp")
            .as_nanos();

        // split to chunks
        let data_slice = split_blob_into_chunks(blob);
        let mut chunks = Vec::with_capacity(data_slice.len());
        for (index, chunk) in data_slice.iter().enumerate() {
            let chunk = BlobChunk {
                digest,
                timestamp,
                index,
                total: data_slice.len(),
                chunk: chunk.to_vec(),
            };
            chunks.push(chunk);
        }
        chunks
    }
}

#[derive(Serialize, Deserialize, Debug, Clone)]
struct RoutingInfo {
    pub slice: u8, // data slice
    pub host_canisters: [Principal; 2],
}

// canister存的时候主要用digest,time用server的time
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct BlobKey {
    pub digest: [u8; 32],
    pub expiry_timestamp: u128,
    pub routing_info: RoutingInfo,
}

#[derive(Clone)]
pub struct ICStorage {
    pub agent: Agent,
    current_index: Option<u8>,
}

impl ICStorage {
    pub fn new(pem_path: String) -> anyhow::Result<Self> {
        let identity = BasicIdentity::from_pem_file(pem_path)?;
        let agent = Agent::builder()
            .with_url("https://ic0.app")
            .with_identity(identity)
            .build()
            .unwrap();
        Ok(Self {
            agent,
            current_index: None,
        })
    }

    pub async fn save_blob(&mut self, blob: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let blob_id = BlobId::new(&blob);
        let key = serde_json::to_string(&blob_id)?;
        info!(
            "ICStorage::save_blob(): blob_id = {blob_id:?}, blob_len = {}",
            blob.len(),
        );

        let chunks = Arc::new(BlobChunk::generate_chunks(&blob));
        let agent = Arc::new(self.agent.clone());
        for cid in self.get_cid()? {
            let _agent = agent.clone();
            let _chunks = chunks.clone();
            let _ = tokio::spawn(Self::push_to_canister(_agent, cid, _chunks)).await?;
        }
        Ok(key.as_bytes().to_vec())
    }

    pub async fn get_blob(&self, key: BlobKey) -> anyhow::Result<Vec<u8>> {
        // inspect expiry timestamp
        let current_timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get timestamp")
            .as_nanos();
        if key.expiry_timestamp < current_timestamp {
            bail!(
                "ICStorage::get_blob(): expired: key.expiry_timestamp = {:?}, current_timestamp = {:?}",
                key.expiry_timestamp,
                current_timestamp
            );
        }

        // get from canisters
        let agent = Arc::new(self.agent.clone());
        let res = Self::get_blob_from_canisters(agent, key.digest, key.routing_info).await?;

        for blob in res {
            let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
            if key.digest == digest {
                return Ok(blob);
            }
        }

        bail!("ICStorage::get_blob(): failed to get blob")
    }

    async fn push_to_canister(
        agent: Arc<Agent>,
        cid: Principal,
        chunks: Arc<Vec<BlobChunk>>,
    ) -> anyhow::Result<()> {
        for chunk in chunks.iter() {
            let fut = || async {
                let res = agent
                    .update(&cid, "save_blob")
                    .with_arg(Encode!(&chunk)?)
                    .call_and_wait()
                    .await;

                match res {
                    Ok(_) => {
                        info!("ICStorage::save_blob_chunk(): success: cid = {cid}");
                        Ok(())
                    }
                    Err(e) => {
                        bail!("ICStorage::save_blob_chunk(): error: {:?}", e);
                    }
                }
            };

            // 5s / retry
            match fut
                .retry(
                    &ExponentialBuilder::default()
                        .with_max_times(3)
                        .with_min_delay(Duration::from_secs(5)),
                )
                .notify(|err: &Error, dur: Duration| {
                    error!(
                        "ICStorage::save_blob(): retrying error {:?} with sleeping {:?}",
                        err, dur
                    );
                })
                .await
            {
                Ok(_) => {}
                Err(e) => {
                    bail!("Failed to exec ICStorage::save_blob(): error: {:?}", e);
                }
            }
        }

        Ok(())
    }

    // get blob from single canister
    // 返回 从2个canister获取的blob，但是不保证获取的是完好的
    // todo canister:  get key: hex encoded digest & slice index
    async fn get_blob_from_canisters(
        agent: Arc<Agent>,
        digest: [u8; 32],
        info: RoutingInfo,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        let hash = Arc::new(hex::encode(digest));

        let mut fut_tasks = Vec::with_capacity(info.host_canisters.len());
        for cid in info.host_canisters {
            // get blobs from single canister
            // return blobs come from different canisters
            let hash = hash.clone();
            let agent = agent.clone();
            let fut = async move {
                let mut tasks = Vec::with_capacity(info.slice as usize);

                // 组装分片的tasks
                for i in 0..info.slice {
                    let _agent = agent.clone();
                    let _hash = hash.clone();
                    let fut = async move {
                        let arg = Encode!(&_hash, &(i as usize)).expect("Failed to encode arg");
                        match _agent.query(&cid, "get_blob").with_arg(arg).call().await {
                            // return blob
                            Ok(res) => Ok(Decode!(&res, Vec<u8>).unwrap()),
                            Err(e) => {
                                bail!("ICStorage::get_blob(): error: {:?}", e);
                            }
                        }
                    };
                    tasks.push(fut);
                }

                let mut chunks = Vec::with_capacity(tasks.len());

                info!("ICStorage::get_blob(): blob digest = {:?}", hash);
                join_all(tasks.into_iter())
                    .await
                    .iter()
                    .for_each(|res| match res {
                        Ok(chunk) => {
                            chunks.push(chunk.to_vec());
                        }
                        Err(e) => {
                            error!("ICStorage::get_blob(): error: {:?}", e);
                        }
                    });
                chunks.concat()
            };
            fut_tasks.push(fut);
        }

        let res = join_all(fut_tasks.into_iter()).await;

        if res.is_empty() {
            bail!("ICStorage::get_blob(): failed to get blob");
        }

        Ok(res)
    }

    fn get_cid(&mut self) -> anyhow::Result<[Principal; 2]> {
        let index = self.current_index.get_or_insert(random::<u8>() % 20);

        let cids = CANISTER_COLLECTIONS.get(*index as usize).unwrap();

        *index += 1;
        *index %= 20;

        Ok([
            Principal::from_text(cids[0]).unwrap(),
            Principal::from_text(cids[1]).unwrap(),
        ])
    }
}

fn split_blob_into_chunks(blob: &[u8]) -> Vec<Vec<u8>> {
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
