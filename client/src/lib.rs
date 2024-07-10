use candid::Principal;
use futures::future::join_all;
use rand::Rng;
use serde_json::json;
use std::collections::HashSet;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info, warn};

use crate::ic_storage::{BlobKey, ICStorage, SIGNATURE_CANISTER};
use crate::signature::{ConfirmationStatus, SignatureCanisterConfig, VerifyResult};
use crate::storage::StorageCanisterConfig;

pub mod ic;
pub mod ic_storage;
pub mod signature;
pub mod storage;

pub async fn get_from_canister(key_path: String, da: &ICStorage) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(key_path)
        .await
        .expect("Unable to open file");

    let mut content = String::new();
    file.read_to_string(&mut content)
        .await
        .expect("Unable to read file");

    let keys: Vec<BlobKey> = serde_json::from_str(&content).unwrap();

    let mut tasks = Vec::new();

    for key in keys.iter() {
        tasks.push(async move {
            match da.get_blob(key.clone()).await {
                Ok(_) => {
                    info!("get from canister success, blob key: {:?}", key);
                }
                Err(e) => error!("get from canister error: {:?}", e),
            }
        });
    }

    join_all(tasks).await;

    Ok(())
}

pub async fn put_to_canister(
    batch_number: usize,
    key_path: String,
    da: &mut ICStorage,
) -> anyhow::Result<()> {
    let mut rng = rand::thread_rng();

    //准备4个blob
    let mut batch = vec![vec![0u8; 3 * 1024 * 1024]; batch_number]; // 10个3M
    for i in &mut batch {
        rng.fill(&mut i[..]);
    }

    let mut keys = Vec::with_capacity(batch.len());

    for (index, item) in batch.iter().enumerate() {
        info!("Batch Index: {}", index);
        let res = da.save_blob(item.to_vec()).await?;
        let raw = String::from_utf8(res).unwrap();
        let key = serde_json::from_str::<BlobKey>(&raw).unwrap();
        keys.push(key)
    }

    let content = fs::read_to_string(&key_path).await.unwrap_or_default();

    let mut old_keys = Vec::new();

    if !content.is_empty() {
        old_keys = serde_json::from_str(content.trim()).unwrap_or_else(|e| {
            error!("parse old keys failed: {}", e);
            Vec::new()
        });
    }

    keys.extend(old_keys);

    let json_value = json!(keys);
    let json_str = serde_json::to_string_pretty(&json_value).unwrap();
    let mut file = OpenOptions::new()
        .write(true)
        .truncate(true)
        .create(true)
        .open(key_path)
        .await
        .expect("Unable to open file");
    // write json str into file
    file.write_all(json_str.as_bytes())
        .await
        .expect("Unable to write file");

    info!("Write key to file success");
    Ok(())
}

pub async fn verify_confirmation(key_path: String, da: &ICStorage) -> anyhow::Result<()> {
    let mut file = OpenOptions::new()
        .read(true)
        .open(key_path)
        .await
        .expect("Unable to open file");

    let mut content = String::new();
    file.read_to_string(&mut content)
        .await
        .expect("Unable to read file");

    let keys: Vec<BlobKey> = serde_json::from_str(&content).unwrap();

    let (tx, mut rx) = tokio::sync::mpsc::channel(keys.len());

    for bk in keys.iter() {
        let _tx = tx.clone();
        let digest = bk.digest;
        let _da = da.clone();
        tokio::spawn(async move {
            let confirmation = _da
                .signature_canister
                .get_confirmation(digest)
                .await
                .unwrap();
            let hexed_digest = hex::encode(digest);
            match _tx.send((hexed_digest, confirmation)).await {
                Ok(_) => {}
                Err(e) => error!("send confirmation failed, error: {}", e),
            }
        });
    }

    // receive channel
    for _ in 0..keys.len() {
        if let Some((hexed_digest, confirmation)) = rx.recv().await {
            match confirmation {
                ConfirmationStatus::Confirmed(confirmation) => {
                    match da
                        .signature_canister
                        .verify_confirmation(&confirmation)
                        .await
                    {
                        VerifyResult::Valid => {
                            info!("confirmation verified, digest: {}", hexed_digest);
                        }
                        VerifyResult::InvalidProof => {
                            error!("confirmation proof is invalid, digest: {}", hexed_digest)
                        }
                        VerifyResult::InvalidSignature(err) => {
                            error!(
                                "confirmation signature is invalid: {}, digest: {}",
                                err, hexed_digest
                            )
                        }
                    }
                }
                ConfirmationStatus::Pending => {
                    warn!("confirmation is pending, digest: {}", hexed_digest)
                }
                ConfirmationStatus::Invalid => {
                    error!("digest is invalid, digest: {}", hexed_digest)
                }
            }
        }
    }

    Ok(())
}

pub async fn init_canister(da: &ICStorage) -> anyhow::Result<()> {
    let owner = da.signature_canister.agent.get_principal().unwrap();

    // update storage canister config:
    let storage_canister_config = StorageCanisterConfig {
        owner,
        signature_canister: Principal::from_text(SIGNATURE_CANISTER).unwrap(),
        query_response_size: 2621440, // query response size, current 2.5 M
        canister_storage_threshold: 6, // every 6 blobs => 1 retired
    };

    let mut tasks = Vec::with_capacity(da.storage_canisters_map.len());
    for (_, s) in da.storage_canisters_map.iter() {
        let _config = storage_canister_config.clone();
        tasks.push(async move {
            match s.update_config(&_config).await {
                Ok(_) => info!(
                    "update storage canister config success, cid: {}",
                    s.canister_id
                ),
                Err(e) => error!(
                    "update storage canister config failed, cid: {}, error: {}",
                    s.canister_id, e
                ),
            }
        });
    }
    join_all(tasks).await;
    info!("updated storage canister config");

    let _ = da.signature_canister.init().await;

    // update signature config: batch confirmation = 1
    let signature_config = SignatureCanisterConfig {
        confirmation_batch_size: 6, // 6 blobs => 1 confirmation
        confirmation_live_time: 1,  // 1 batch 1 expired
        da_canisters: HashSet::from_iter(da.storage_canisters_map.keys().copied()),
        owner,
    };
    match da.signature_canister.update_config(&signature_config).await {
        Ok(_) => info!("update signature config success"),
        Err(e) => error!("update signature config failed: {}", e),
    }
    info!("signature canister initialized and updated");

    Ok(())
}
