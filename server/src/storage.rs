//! Storage backend for the DA server.

use anyhow::{bail, Result};
use async_trait::async_trait;
use aws_sdk_s3::Client;
use candid::Deserialize;
use icda_core::icda::{BlobKey, BLOB_LIVE_TIME, ICDA, REPLICA_NUM};
use redb::{Database, Durability, ReadableTable, TableDefinition as TblDef};
use serde::Serialize;
use sha2::Digest;
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{error, info};

/// Key: BlobId in JSON string format
/// Value: Blob
const BLOBS: TblDef<&str, Vec<u8>> = TblDef::new("da_server_blobs");

/// Blob identifier.
#[derive(Serialize, Deserialize, Debug)]
struct BlobId {
    /// Sha256 digest of the blob in hex format.
    pub(crate) digest: [u8; 32],

    /// Time since epoch in nanos.
    pub(crate) timestamp: u128,
}

impl BlobId {
    /// Creates the blob id for the blob.
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

#[async_trait]
impl Storage for ICDA {
    async fn save_blob(&self, blob: Vec<u8>) -> Result<Vec<u8>> {
        let blob_key = self.push_blob_to_canisters(blob).await?;

        let key = serde_json::to_vec(&blob_key)?;
        Ok(key)
    }

    // ATTENTION: the blob id type is BlobKey
    async fn get_blob(&self, blob_id: Vec<u8>) -> Result<Vec<u8>> {
        let key = serde_json::from_slice::<BlobKey>(&blob_id)?;
        self.get_blob_from_canisters(key).await
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
}
