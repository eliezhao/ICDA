//! Storage backend for the DA server.

use anyhow::{bail, Error, Result};
use async_trait::async_trait;
use aws_sdk_s3::primitives::Blob;
use aws_sdk_s3::Client;
use backon::{ExponentialBuilder, Retryable};
use candid::{Encode, Principal};
use clap::arg;
use futures::future::join_all;
use ic_agent::identity::BasicIdentity;
use ic_agent::{Agent, AgentError};
use redb::{Database, Durability, ReadableTable, TableDefinition as TblDef};
use sha2::Digest;
use std::sync::Arc;
use std::time::Duration;
use tracing::{error, info, warn};

use crate::BlobId;

/// IC Storage

/// Key: BlobId in JSON string format
/// Value: Blob
const BLOBS: TblDef<&str, Vec<u8>> = TblDef::new("da_server_blobs");

const CANISTER_COLLECTIONS: [[Principal; 2]; 20] = [[Principal::anonymous(); 2]; 20]; // todo: 等待创建

const BASIC_TIMESTAMP: u128 = 0; // start time stamp

#[async_trait]
pub trait Storage: Send + Sync {
    /// Saves the blob.
    /// Returns the BlobId as bytes.
    async fn save_blob(&self, blob: Vec<u8>) -> Result<Vec<u8>>;

    /// Retrieves the blob.
    async fn get_blob(&self, blob_id: Vec<u8>) -> Result<Vec<u8>>;
}

/// Storage implementation with S3 as backend.
pub struct S3Storage {
    /// Client handle.
    client: Client,

    /// S3 bucket
    bucket: String,
}

impl S3Storage {
    ///  Sets up the client interface.
    pub async fn new(profile: String, bucket: String) -> Self {
        let config = aws_config::from_env().profile_name(profile).load().await;
        Self {
            client: Client::new(&config),
            bucket,
        }
    }
}

#[async_trait]
impl Storage for S3Storage {
    async fn save_blob(&self, blob: Vec<u8>) -> Result<Vec<u8>> {
        let blob_id = BlobId::new(&blob);
        let key = serde_json::to_string(&blob_id)?;
        tracing::info!(
            "S3Storage::save_blob(): blob_id = {blob_id:?}, blob_len = {}",
            blob.len(),
        );
        self.client
            .put_object()
            .bucket(&self.bucket)
            .key(&key)
            .body(blob.into())
            .send()
            .await
            .map(|_| key.as_bytes().to_vec())
            .map_err(|err| err.into())
    }

    async fn get_blob(&self, blob_id: Vec<u8>) -> Result<Vec<u8>> {
        let key = String::from_utf8(blob_id)?;
        let blob_id: BlobId = serde_json::from_str(&key)?;
        tracing::info!("S3Storage::get_blob(): blob_id = {blob_id:?}");

        let resp = self
            .client
            .get_object()
            .bucket(&self.bucket)
            .key(&key)
            .send()
            .await?;
        let blob = resp.body.collect().await?.to_vec();
        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        if blob_id.digest != digest {
            bail!(
                "S3Storage: digest mismatch: blob_id.digest = {:?}, actual = {digest:?}",
                blob_id.digest
            );
        }
        Ok(blob)
    }
}

/// Storage implementation with redb backend.
pub struct LocalStorage {
    /// The redb database.
    db: Database,
}

impl LocalStorage {
    /// Sets up the DB.
    pub fn new(db_path: impl AsRef<std::path::Path>) -> Result<Self, redb::Error> {
        let db = redb::Database::builder().create(db_path)?;
        let mut tx = db.begin_write()?;
        let table = tx.open_table(BLOBS)?;
        drop(table);
        tx.set_durability(Durability::Immediate);
        tx.commit()?;

        Ok(Self { db })
    }
}

#[async_trait]
impl Storage for LocalStorage {
    async fn save_blob(&self, blob: Vec<u8>) -> Result<Vec<u8>> {
        let blob_id = BlobId::new(&blob);
        let key = serde_json::to_string(&blob_id)?;
        tracing::info!(
            "LocalStorage::save_blob(): blob_id = {blob_id:?}, blob_len = {}",
            blob.len(),
        );

        // Insert into the table.
        let mut tx = self.db.begin_write()?;
        let mut table = tx.open_table(BLOBS)?;
        table.insert(key.as_str(), blob)?;
        drop(table);
        tx.set_durability(redb::Durability::Immediate);
        tx.commit()?;

        Ok(key.as_bytes().to_vec())
    }

    async fn get_blob(&self, blob_id: Vec<u8>) -> Result<Vec<u8>> {
        let key = String::from_utf8(blob_id)?;
        let blob_id: BlobId = serde_json::from_str(&key)?;
        tracing::info!("LocalStorage::get_blob(): blob_id = {blob_id:?}");

        // Read from the table.
        let tx = self.db.begin_read()?;
        let table = tx.open_table(BLOBS)?;
        let blob = match table.get(key.as_str())?.map(|blob| blob.value()) {
            Some(blob) => blob,
            None => {
                bail!("LocalStorage::get_blob(): blob not found: {blob_id:?}",);
            }
        };

        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        if blob_id.digest != digest {
            bail!(
                "LocalStorage::get_blob(): digest mismatch: blob_id.digest = {:?}, actual = {digest:?}",
                blob_id.digest
            );
        }
        Ok(blob)
    }
}

#[derive(Clone)]
pub struct ICStorage {
    agent: Agent,
}

impl ICStorage {
    pub fn new(pem_path: String) -> Result<Self> {
        let identity = BasicIdentity::from_pem_file(pem_path)?;
        let agent = Agent::builder()
            .with_url("https://ic0.app")
            .with_identity(identity)
            .build()
            .unwrap();
        Ok(Self { agent })
    }
}

#[async_trait]
impl Storage for ICStorage {
    async fn save_blob(&self, blob: Vec<u8>) -> Result<Vec<u8>> {
        let blob_id = BlobId::new(&blob);
        let key = serde_json::to_string(&blob_id)?;
        tracing::info!(
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

    async fn get_blob(&self, blob_id: Vec<u8>) -> Result<Vec<u8>> {
        let key = String::from_utf8(blob_id)?;
        let blob_id: BlobId = serde_json::from_str(&key)?;
        let cids = Self::get_cid(&blob_id)?;

        tracing::info!("ICStorage::get_blob(): blob_id = {blob_id:?}");

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
                    warn!("ICStorage::get_blob(): error: {:?}", e);
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
}

impl ICStorage {
    fn get_cid(blob_id: &BlobId) -> Result<[Principal; 2]> {
        let batch_number = (blob_id.timestamp - BASIC_TIMESTAMP) / 12;

        let batch_index = batch_number % 20;

        Ok(*CANISTER_COLLECTIONS.get(batch_index as usize).unwrap())
    }

    async fn push_to_canister(agent: Arc<Agent>, cid: Principal, arg: Arc<Vec<u8>>) -> Result<()> {
        let fut = || async {
            let res = agent
                .update(&cid, "save_blob")
                .with_arg(arg.to_vec())
                .call_and_wait()
                .await;

            match res {
                Ok(_) => Ok(()),
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
            warn!(
                "ICStorage::save_blob(): retrying error {:?} with sleeping {:?}",
                err, dur
            );
        })
        .await
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use ic_agent::identity::BasicIdentity;

    async fn test_storage(storage: &dyn Storage) {
        let blob1 = vec![1; 32];
        let blob_id_1 = storage.save_blob(blob1.clone()).await.unwrap();
        let ret = storage.get_blob(blob_id_1.clone()).await.unwrap();
        assert_eq!(ret, blob1);

        let blob2 = vec![3; 64];
        let blob_id_2 = storage.save_blob(blob2.clone()).await.unwrap();
        let ret = storage.get_blob(blob_id_2.clone()).await.unwrap();
        assert_eq!(ret, blob2);

        // Non-existent blob.
        let blob3 = vec![5; 128];
        let blob_id_3 = BlobId::new(&blob3);
        let key = serde_json::to_string(&blob_id_3).unwrap();
        let key = key.as_bytes().to_vec();
        assert!(storage.get_blob(key).await.is_err());
    }

    #[tokio::test]
    async fn test_local_storage() {
        let tmp_dir = tempfile::tempdir().unwrap();
        let db_path = tmp_dir.path().join("da_server_blob.db");
        let storage = LocalStorage::new(db_path).unwrap();
        test_storage(&storage).await;
    }

    #[tokio::test]
    #[ignore = "Needs AWS set up"]
    async fn test_s3_storage() {
        let storage = S3Storage::new("test-region".into(), "test-bucket".into()).await;
        test_storage(&storage).await;
    }

    #[tokio::test]
    #[ignore = "Needs Canister set up"]
    async fn test_ic_storage() {
        let storage = S3Storage::new("test-region".into(), "test-bucket".into()).await;
        let identity = BasicIdentity::from_pem_file("").expect("Failed to load identity");
        let agent = Agent::builder().with_identity(identity).build().unwrap();
        todo!("需要先创建canister")
        //let storage = ICStorage::new(vec![[]], agent);
        //test_storage(&storage).await;
    }
}
