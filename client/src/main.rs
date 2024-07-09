//! 1. 直接和canister交互
//! 2. 直接和server交互
//! 3. 功能为put和get

extern crate core;

use anyhow::Result;
use clap::{Parser, ValueEnum};
use tokio::fs;
use tracing::{info, Level};

use client::canister_interface::ICStorage;
use client::{get_from_canister, put_to_canister, verify_confirmation};

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
    Verify,
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
        Action::Verify => {
            let _ = verify_confirmation(key_path, &da).await;
        }
    }
}

async fn talk_to_server(_ip: String) {}
