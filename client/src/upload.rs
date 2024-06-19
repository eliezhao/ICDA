use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use anyhow::bail;
use candid::utils::ArgumentEncoder;
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use futures::future::join_all;
use ic_agent::agent::Transport;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::random;
use serde::Serialize;
use sha2::Digest;

// 20 subnets with 40 canisters
const CANISTER_COLLECTIONS: [[&str; 2]; 20] =
    [["hxctj-oiaaa-aaaap-qhltq-cai", "v3y75-6iaaa-aaaak-qikaa-cai"]; 20]; // 测试用 canisters

const CHUNK_SIZE: usize = 1 << 20; // 1 MB

const LIVE_TIME: u128 = 60 * 60 * 24 * 7 * 1_000_000_000; // 1 week in nanos

#[derive(Deserialize, Serialize, CandidType, Debug)]
struct BlobChunk {
    /// Sha256 digest of the blob in hex format.
    /// hex encoded digest
    digest: String,

    /// Time since epoch in nanos.
    timestamp: u128,

    /// Total number of chunks.
    total: usize,

    /// The actual chunk.
    chunk: Vec<u8>,
}

impl BlobChunk {
    pub fn generate_chunks(blob: &[u8], digest: String, timestamp: u128) -> Vec<BlobChunk> {
        // split to chunks
        let data_slice = Self::split_blob_into_chunks(blob);
        let mut chunks = Vec::with_capacity(data_slice.len());
        for slice in data_slice.iter() {
            let chunk = BlobChunk {
                digest: digest.clone(),
                timestamp,
                total: blob.len(),
                chunk: slice.to_vec(),
            };
            chunks.push(chunk);
        }
        chunks
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
}

#[derive(Serialize, Deserialize, Clone)]
pub struct RoutingInfo {
    pub total_size: usize,
    pub host_canisters: [Principal; 2],
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

impl BlobKey {
    pub fn new(blob: &Vec<u8>) -> Self {
        Self {
            digest: sha2::Sha256::digest(blob).into(),
            expiry_timestamp: SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Failed to get timestamp")
                .as_nanos()
                + LIVE_TIME,
            routing_info: RoutingInfo {
                total_size: 0,
                host_canisters: [Principal::anonymous(), Principal::anonymous()],
            },
        }
    }
}

#[derive(Clone)]
pub struct ICStorage {
    pub agent: Agent,
    current_index: Option<u8>,
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
struct Blob {
    data: Vec<u8>,
    next: Option<usize>, // next start index
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
        let routing_canisters = self.get_cid()?;
        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        let hex_digest = hex::encode(digest);
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get timestamp")
            .as_nanos();

        let chunks = Arc::new(BlobChunk::generate_chunks(&blob, hex_digest, timestamp));
        let agent = Arc::new(self.agent.clone());

        for cid in routing_canisters.iter() {
            let _agent = agent.clone();
            let _chunks = chunks.clone();
            tokio::spawn(Self::push_to_canister(_agent, cid.clone(), _chunks));
        }

        let blob_key = BlobKey {
            digest,
            expiry_timestamp: timestamp + LIVE_TIME,
            routing_info: RoutingInfo {
                total_size: blob.len(),
                host_canisters: routing_canisters,
            },
        };

        println!("ICStorage::save_blob(): key = {:?}", blob_key);
        let key = serde_json::to_string(&blob_key)?;
        Ok(key.as_bytes().to_vec())
    }

    async fn push_to_canister(
        agent: Arc<Agent>,
        cid: Principal,
        chunks: Arc<Vec<BlobChunk>>,
    ) -> anyhow::Result<()> {
        for (index, chunk) in chunks.iter().enumerate() {
            println!(
                "begin push hash: {}, index: {} to canister: {}",
                chunk.digest,
                index,
                cid.to_text()
            );
            let res = agent
                .update(&cid, "save_blob")
                .with_arg(Encode!(&chunk)?)
                .call_and_wait()
                .await?;

            let res = Decode!(&res, Result<(), String>)?;

            match res {
                Ok(_) => {
                    println!("ICStorage::save_blob_chunk(): success: cid = {cid}, index = {index}");
                }
                Err(e) => {
                    bail!(
                        "ICStorage::save_blob_chunk(): index = {index}, cid = {cid}, error: {:?}",
                        e
                    );
                }
            }
        }

        Ok(())
    }

    pub async fn get_blob(&self, key: BlobKey) -> anyhow::Result<Vec<u8>> {
        // inspect expiry timestamp
        let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        if key.expiry_timestamp < current_timestamp {
            bail!(
                "ICStorage::get_blob(): expired: key.expiry_timestamp = {:?}, current_timestamp = {:?}",
                key.expiry_timestamp,
                current_timestamp
            );
        }

        // get from canisters
        let agent = Arc::new(self.agent.clone());
        let res = Self::get_blob_from_canisters(agent, &key).await?;

        for blob in res {
            let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
            if key.digest == digest {
                println!("ICStorage::get_blob(): get blob success blob digest match:\nkey digest:{:?}\nget blob digest{:?}", hex::encode(key.digest), hex::encode(digest));
                return Ok(blob);
            } else {
                println!("ICStorage::get_blob(): blob digest not match\nkey digest:{:?}\nget blob digest{:?}", hex::encode(key.digest), hex::encode(digest));
            }
        }

        bail!("ICStorage::get_blob(): failed to get blob")
    }

    // get blob from single canister
    // 返回 从2个canister获取的blob，但是不保证获取的是完好的
    async fn get_blob_from_canisters(
        agent: Arc<Agent>,
        key: &BlobKey,
    ) -> anyhow::Result<Vec<Vec<u8>>> {
        let hash = Arc::new(hex::encode(key.digest));

        let mut fut_tasks = Vec::with_capacity(key.routing_info.host_canisters.len());
        for cid in key.routing_info.host_canisters {
            // get blobs from single canister
            // return blobs come from different canisters
            let _hash = hash.clone();
            let agent = agent.clone();
            let fut = async move {
                // 创建一样大小的buffer
                let mut blob = Vec::with_capacity(key.routing_info.total_size);
                println!("ICStorage::get_blob(): blob digest = {:?}", _hash);

                let res = agent
                    .query(&cid, "get_blob")
                    .with_arg(Encode!(&_hash).unwrap())
                    .call()
                    .await
                    .expect("Failed to query first slice");
                let mut slice = Decode!(&res, Blob).expect("Failed to decode blob first slice");
                println!(
                    "get blob size = {}, next: {:?}",
                    slice.data.len(),
                    slice.next
                );
                blob.extend(slice.data);

                while let Some(next_index) = slice.next {
                    // get blob by index
                    println!(
                        "ICStorage::get_blob(): continue get blob digest = {:?}",
                        _hash
                    );
                    let res = agent
                        .query(&cid, "get_blob_with_index")
                        .with_arg(Encode!(&_hash, &next_index).unwrap())
                        .call()
                        .await
                        .expect("Failed to query next slice");
                    slice = Decode!(&res, Blob).expect(
                        format!(
                            "Failed to decode blob next slice, key: {}, index: {}",
                            _hash, next_index
                        )
                        .as_str(),
                    );
                    blob.extend(slice.data);
                }
                blob
            };
            fut_tasks.push(fut);
        }

        let res = join_all(fut_tasks.into_iter()).await;

        if res.is_empty() {
            bail!("get_blob(): failed to get blob");
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
