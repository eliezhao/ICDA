use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use anyhow::{bail, Error};
use backon::{ExponentialBuilder, Retryable};
use candid::{Deserialize, Encode, Principal};
use futures::future::join_all;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use serde::Serialize;
use sha2::Digest;

const CANISTER_COLLECTIONS: [[&str; 2]; 20] =
    [["hxctj-oiaaa-aaaap-qhltq-cai", "v3y75-6iaaa-aaaak-qikaa-cai"]; 20]; // 测试用 canisters

const BASIC_TIMESTAMP: u128 = 0; // start time stamp

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

#[derive(Clone)]
pub struct ICStorage {
    pub agent: Agent,
}

impl ICStorage {
    pub fn new(pem_path: String) -> anyhow::Result<Self> {
        let identity = BasicIdentity::from_pem_file(pem_path)?;
        let agent = Agent::builder()
            .with_url("https://ic0.app")
            .with_identity(identity)
            .build()
            .unwrap();
        Ok(Self { agent })
    }
    pub async fn save_blob(&self, blob: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let blob_id = BlobId::new(&blob);
        let key = serde_json::to_string(&blob_id)?;
        println!(
            "ICStorage::save_blob(): blob_id = {blob_id:?}, blob_len = {}",
            blob.len(),
        );

        let arg = Arc::new(Encode!(&key, &blob)?);
        let agent = Arc::new(self.agent.clone());
        for cid in Self::get_cid(&blob_id)? {
            let _agent = agent.clone();
            let _arg = arg.clone();
            let _ = tokio::spawn(Self::push_to_canister(_agent, cid, _arg)).await?;
        }
        Ok(key.as_bytes().to_vec())
    }

    pub async fn get_blob(&self, blob_id: Vec<u8>) -> anyhow::Result<Vec<u8>> {
        let key = String::from_utf8(blob_id)?;
        let blob_id: BlobId = serde_json::from_str(&key)?;
        let cids = Self::get_cid(&blob_id)?;

        println!("ICStorage::get_blob(): blob_id = {blob_id:?}");

        let arg = Arc::new(Encode!(&key)?);
        let agent = Arc::new(self.agent.clone());
        let mut tasks = Vec::new();
        for cid in cids {
            let _agent = agent.clone();
            let _arg = arg.clone();
            tasks.push(async move {
                _agent
                    .query(&cid, "get_blob")
                    .with_arg(_arg.to_vec())
                    .call()
                    .await
            });
        }

        let res = join_all(tasks.into_iter())
            .await
            .iter()
            .filter_map(|res| match res {
                Ok(blob) => Some(blob.to_vec()),
                Err(e) => {
                    println!("ICStorage::get_blob(): error: {:?}", e);
                    None
                }
            })
            .collect::<Vec<_>>();

        if res.len() == 0 {
            bail!("ICStorage::get_blob(): all queries failed: {res:?}");
        }

        let blob = res.get(0).unwrap();
        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        if blob_id.digest != digest {
            bail!(
                "ICStorage: digest mismatch: blob_id.digest = {:?}, actual = {digest:?}",
                blob_id.digest
            );
        }

        Ok(blob.to_vec())
    }

    fn get_cid(blob_id: &BlobId) -> anyhow::Result<[Principal; 2]> {
        let batch_number = (blob_id.timestamp - BASIC_TIMESTAMP) / 12;

        let batch_index = batch_number % 20;
        println!(
            "Blob ID: {:?}, \nBatch Number: {}, \nBatch Index: {}",
            blob_id, batch_number, batch_index
        );
        let cids = CANISTER_COLLECTIONS.get(batch_index as usize).unwrap();

        Ok([
            Principal::from_text(cids[0]).unwrap(),
            Principal::from_text(cids[1]).unwrap(),
        ])
    }

    async fn push_to_canister(
        agent: Arc<Agent>,
        cid: Principal,
        arg: Arc<Vec<u8>>,
    ) -> anyhow::Result<()> {
        let fut = || async {
            let res = agent
                .update(&cid, "save_blob")
                .with_arg(arg.to_vec())
                .call_and_wait()
                .await;

            match res {
                Ok(_) => Ok(println!("ICStorage::save_blob(): success: cid = {cid}")),
                Err(e) => {
                    bail!("ICStorage::save_blob(): error: {:?}", e);
                }
            }
        };

        // 5s / retry
        fut.retry(
            &ExponentialBuilder::default()
                .with_max_times(3)
                .with_min_delay(Duration::from_secs(5)),
        )
        .notify(|err: &Error, dur: Duration| {
            println!(
                "ICStorage::save_blob(): retrying error {:?} with sleeping {:?}",
                err, dur
            );
        })
        .await
    }
}
