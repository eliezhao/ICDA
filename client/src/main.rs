//! 1. 直接和canister交互
//! 2. 直接和server交互
//! 3. 功能为put和get

extern crate core;

use std::io::Write;

use anyhow::Result;
use candid::{CandidType, Deserialize};
use clap::{Parser, Subcommand, ValueEnum};
use rand::Rng;
use serde_json::json;
use sha2::Digest;
use tokio::fs;
use tokio::fs::OpenOptions;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tracing::{info, Level};
use tracing_subscriber::fmt;

use client::canister_interface::{BlobKey, ICStorage};

const E8S: u64 = 100_000_000;

#[derive(Parser)]
struct Cli {
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[arg(short, long)]
    action: Action,
}

// ./client action put --config "config.toml"
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

async fn talk_to_server(ip: String) {}
