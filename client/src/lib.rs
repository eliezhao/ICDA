use std::collections::HashSet;
use std::sync::Arc;

use candid::Principal;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::Rng;
use serde_json::json;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{error, info, warn};

use crate::canister_interface::{
    BlobKey, ICStorage, CANISTER_COLLECTIONS, CONFIRMATION_BATCH_SIZE, CONFIRMATION_LIVE_TIME,
    SIGNATURE_CANISTER,
};
use crate::signature::{ConfirmationStatus, SignatureCanister};

pub mod canister_interface;
pub mod ic;
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

    for (index, key) in keys.iter().enumerate() {
        info!("Batch Index: {}", index);
        match da.get_blob(key.clone()).await {
            Ok(_) => {}
            Err(e) => error!("get from canister error: {:?}", e),
        };
    }
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

    let mut response = Vec::new();

    for (index, item) in batch.iter().enumerate() {
        info!("Batch Index: {}", index);
        let res = da.save_blob(item.to_vec()).await?;
        let raw = String::from_utf8(res).unwrap();
        let key = serde_json::from_str::<BlobKey>(&raw).unwrap();
        response.push(key)
    }

    let json_value = json!(response);
    let json_str = serde_json::to_string_pretty(&json_value).unwrap();

    let mut file = OpenOptions::new()
        .write(true)
        .create(true)
        .truncate(true)
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

    let sc = da.signature_canister.clone();

    for (index, key) in keys.iter().enumerate() {
        info!("Batch Index: {}", index);
        let confirmation = sc.get_confirmation(key.digest).await.unwrap();
        match confirmation {
            ConfirmationStatus::Confirmed(confirmation) => {
                if sc.verify_confirmation(&confirmation).await {
                    info!("confirmation verified, digest: {}", hex::encode(key.digest));
                } else {
                    error!("confirmation invalid, digest: {}", hex::encode(key.digest));
                }
            }
            ConfirmationStatus::Pending => {
                warn!("confirmation is pending")
            }
            ConfirmationStatus::Invalid => {
                error!("digest is invalid")
            }
        }
    }

    Ok(())
}

async fn init_signature_canister(pem_path: String) -> anyhow::Result<()> {
    let identity = BasicIdentity::from_pem_file(pem_path)?;
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .with_identity(identity)
        .build()
        .unwrap();
    let owner = agent.get_principal().unwrap();
    let da_canisters = HashSet::from_iter(
        CANISTER_COLLECTIONS
            .iter()
            .map(|x| {
                x.iter()
                    .map(|x| Principal::from_text(x).unwrap())
                    .collect::<Vec<Principal>>()
            })
            .collect::<Vec<_>>()
            .concat(),
    );
    let confirmation_live_time = CONFIRMATION_LIVE_TIME;
    let confirmation_batch_size = CONFIRMATION_BATCH_SIZE;
    let config = signature::SignatureCanisterConfig {
        owner,
        da_canisters,
        confirmation_live_time,
        confirmation_batch_size,
    };

    let signature_canister_id = Principal::from_text(SIGNATURE_CANISTER).unwrap();
    let signature_canister = SignatureCanister::new(signature_canister_id, Arc::new(agent));
    signature_canister.update_config(&config).await?;

    Ok(())
}
