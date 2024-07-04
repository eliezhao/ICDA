//! 1. 直接和canister交互
//! 2. 直接和server交互
//! 3. 功能为put和get

extern crate core;

use std::collections::HashSet;
use std::sync::Arc;

use anyhow::Result;
use candid::Principal;
use clap::{Parser, ValueEnum};
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::Rng;
use serde_json::json;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, Level};

use client::canister_interface::{
    BlobKey, ICStorage, CANISTER_COLLECTIONS, CONFIRMATION_BATCH_SIZE, CONFIRMATION_LIVE_TIME,
    SIGNATURE_CANISTER,
};
use client::signature;
use client::signature::{ConfirmationStatus, SignatureCanister};

#[warn(dead_code)]
const E8S: u64 = 100_000_000;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[arg(short, long)]
    action: Action,
}

#[derive(serde::Deserialize, Debug)]
struct Config {
    identity: String,
    mode: Mode,
    // test use
    batch_number: usize,
    blob_key: String,
}

#[derive(serde::Deserialize, Debug)]
enum Mode {
    Canister,
    Server { ip: String }, // ipv4
}

// put get config default : keys.json
#[derive(serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, ValueEnum)]
enum Action {
    Put,
    Get,
}

#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let cli = Cli::parse();
    let content = fs::read_to_string(cli.config).await?;
    let config: Config = toml::from_str(&content)?;

    info!("Start client with config: {:?}", config);

    match config.mode {
        Mode::Canister => {
            talk_to_canister(
                config.identity,
                config.batch_number,
                config.blob_key,
                cli.action,
            )
            .await;
        }
        Mode::Server { ip } => {
            talk_to_server(ip).await;
        }
    }

    Ok(())
}

async fn init_signature_canister(pem_path: String) -> Result<()> {
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
    let config = signature::Config {
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

async fn talk_to_canister(
    identity_path: String,
    batch_number: usize,
    key_path: String,
    action: Action,
) {
    let mut da = ICStorage::new(&identity_path).unwrap();

    match action {
        Action::Put => {
            let _ = put_to_canister(batch_number, key_path, &mut da).await;
        }
        Action::Get => {
            let _ = get_from_canister(key_path, &da).await;
        }
    }
}

async fn get_from_canister(key_path: String, da: &ICStorage) -> Result<()> {
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
        let _ = da.get_blob(key.clone()).await.unwrap();
    }
    Ok(())
}

async fn put_to_canister(batch_number: usize, key_path: String, da: &mut ICStorage) -> Result<()> {
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
    let json_str = serde_json::to_string(&json_value).unwrap();

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

async fn verify_confirmation(key_path: String) -> Result<()> {
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

    let ics = ICStorage::new("../bin/identity.pem").unwrap();

    let sc = ics.signature_canister.clone();

    for (index, key) in keys.iter().enumerate() {
        info!("Batch Index: {}", index);
        let confirmation = sc.get_confirmation(key.digest).await.unwrap();
        match confirmation {
            ConfirmationStatus::Confirmed(confirmation) => {
                if sc.verify_confirmation(&confirmation).await {
                    println!("confirmation verified, digest: {}", hex::encode(key.digest));
                } else {
                    println!("confirmation invalid, digest: {}", hex::encode(key.digest));
                }
            }
            ConfirmationStatus::Pending => {
                println!("confirmation is pending")
            }
            ConfirmationStatus::Invalid => {
                println!("digest is invalid")
            }
        }
    }

    Ok(())
}

async fn talk_to_server(_ip: String) {}

#[tokio::test]
async fn test() {
    let _ = verify_confirmation("../bin/blob_key.".to_string()).await;
}
