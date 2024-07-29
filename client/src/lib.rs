use futures::future::join_all;
use icda_core::canister_interface::signature::{
    ConfirmationStatus, SignatureCanisterConfig, VerifyResult,
};
use icda_core::canister_interface::storage::StorageCanisterConfig;
use icda_core::icda::{BlobKey, ICDA};
use rand::Rng;
use serde_json::json;
use std::sync::Arc;
use std::time::Duration;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub mod ic;

pub async fn get_from_canister(key_path: String, da: ICDA) -> anyhow::Result<()> {
    let da = Arc::new(da);

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
        let _da = da.clone();
        tasks.push(async move {
            match _da.get_blob_from_canisters(key.clone()).await {
                Ok(_) => {
                    info!("get from canister success, blob key: \n{:?}", key);
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
    da: ICDA,
) -> anyhow::Result<()> {
    let keys = Arc::new(Mutex::new(Vec::with_capacity(5 * batch_number)));
    // future tasks
    let futs = Arc::new(Mutex::new(Vec::with_capacity(batch_number)));

    let da = Arc::new(da);
    // push blob to canisters
    for i in 0..batch_number {
        info!("Batch Index: {}", i);

        let _keys = keys.clone();
        let _da = da.clone();
        let _futs = futs.clone();
        // send 10M futures
        let mut batch = vec![vec![0u8; 2 * 1024 * 1024]; 5];
        let mut rng = rand::thread_rng();
        for sub in &mut batch {
            rng.fill(&mut sub[..]);
        }

        let fut = async move {
            for item in batch.into_iter() {
                let _da = _da.clone();
                let _keys = _keys.clone();
                let disperse_fut = async move {
                    match _da.push_blob_to_canisters(item).await {
                        Ok(res) => {
                            info!("push blob to canister success, blob key: \n{:?}", res);
                            _keys.lock().await.push(res);
                        }
                        Err(e) => {
                            error!("push blob to canister error: {:?}", e);
                        }
                    }
                };
                let handle = tokio::spawn(disperse_fut);
                _futs.lock().await.push(handle);
            }
            drop(_futs);
        };

        tokio::spawn(fut);
        tokio::time::sleep(Duration::from_secs(15)).await;
    }

    while Arc::strong_count(&futs) > 1 {
        info!("waiting for all futures to complete");
        tokio::time::sleep(Duration::from_secs(5)).await;
    }

    let mut futs_guard = futs.lock().await;
    let futs_vec = futs_guard.drain(..).collect::<Vec<_>>();
    drop(futs_guard); // Explicitly drop the lock to avoid holding it during await
    let _ = join_all(futs_vec).await;

    let content = fs::read_to_string(&key_path).await.unwrap_or_default();

    let mut old_keys = Vec::new();

    if !content.is_empty() {
        old_keys = serde_json::from_str(content.trim()).unwrap_or_else(|e| {
            error!("parse old keys failed: {}", e);
            Vec::new()
        });
    }

    keys.lock().await.extend(old_keys);

    let json_value = json!(*(keys.lock().await));
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

pub async fn verify_confirmation(key_path: String, da: ICDA) -> anyhow::Result<()> {
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

#[derive(serde::Deserialize, serde::Serialize)]
struct InitConfig {
    #[serde(rename = "storage")]
    storage_config: Option<StorageCanisterConfig>,
    #[serde(rename = "signature")]
    signature_config: Option<SignatureCanisterConfig>,
}

pub async fn init_canister(config_path: String, da: &ICDA) -> anyhow::Result<()> {
    let content = fs::read_to_string(config_path).await?;
    let config: InitConfig = toml::from_str(&content)?;

    let storage_canister_config = config.storage_config.unwrap_or_default();
    let signature_canister_config = config.signature_config.unwrap_or_default();

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

    match da
        .signature_canister
        .update_config(&signature_canister_config)
        .await
    {
        Ok(_) => info!("update signature config success"),
        Err(e) => error!("update signature config failed: {}", e),
    }
    info!("signature canister initialized and updated");

    Ok(())
}
