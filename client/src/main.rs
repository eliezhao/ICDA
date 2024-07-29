//! 1. 直接和canister交互
//! 2. 直接和server交互
//! 3. 功能为put和get

extern crate core;

use anyhow::Result;
use clap::{Parser, Subcommand};
use tokio::fs;
use tracing::{info, Level};

use client::{get_from_canister, init_canister, put_to_canister, verify_confirmation};
use icda_core::icda::ICDA;

#[derive(Parser)]
#[command(name = "client")]
struct Cli {
    #[arg(short, long, default_value = "config.toml")]
    config: String,

    #[command(subcommand)]
    commands: Commands,
}

#[derive(Subcommand)]
enum Commands {
    #[command(name = "put")]
    Put,
    #[command(name = "get")]
    Get,
    #[command(name = "verify")]
    Verify,
    #[command(name = "init")]
    Init(InitConfigPath),
}

#[derive(serde::Deserialize, Clone, PartialEq, Eq, PartialOrd, Ord, Parser)]
struct InitConfigPath {
    #[arg(long, short)]
    path: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct Config {
    identity: IdentityConfig,
    batch: BatchConfig,
    #[serde(rename = "blobkey")]
    blob_key: BlobKeyConfig,
    mode: Option<Mode>,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
enum Mode {
    Canister,
    Server { ip: String }, // ipv4
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct IdentityConfig {
    path: String,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct BatchConfig {
    batch_number: usize,
}

#[derive(serde::Deserialize, serde::Serialize, Debug)]
struct BlobKeyConfig {
    path: String,
}

// put get config default : keys.json
#[tokio::main]
async fn main() -> Result<()> {
    tracing_subscriber::fmt().with_max_level(Level::INFO).init();

    let cli = Cli::parse();
    let content = fs::read_to_string(cli.config).await?;
    let config: Config = toml::from_str(&content)?;

    info!("Start client with config: {:?}", config);

    if let Some(mode) = config.mode {
        match mode {
            Mode::Canister => {
                info!("Mode: Canister");
                talk_to_canister(
                    config.identity.path,
                    config.batch.batch_number,
                    config.blob_key.path,
                    cli.commands,
                )
                .await;
            }
            Mode::Server { ip } => {
                info!("Mode: Server, ip: {}", ip);
                talk_to_server(ip).await;
            }
        }
    } else {
        talk_to_canister(
            config.identity.path,
            config.batch.batch_number,
            config.blob_key.path,
            cli.commands,
        )
        .await;
    }

    Ok(())
}

async fn talk_to_canister(
    identity_path: String,
    batch_number: usize,
    key_path: String,
    commands: Commands,
) {
    let da = ICDA::new(identity_path).await.unwrap();

    match commands {
        Commands::Put => {
            let _ = put_to_canister(batch_number, key_path, da).await;
        }
        Commands::Get => {
            let _ = get_from_canister(key_path, da).await;
        }
        Commands::Verify => {
            let _ = verify_confirmation(key_path, da).await;
        }
        Commands::Init(InitConfigPath { path }) => {
            let _ = init_canister(path, &da).await;
        }
    }
}

async fn talk_to_server(_ip: String) {}
