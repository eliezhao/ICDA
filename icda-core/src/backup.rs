use crate::canister_interface::storage::BlobChunk;
use crate::icda::ICDA;
use candid::Principal;
use regex::Regex;
use std::path::PathBuf;
use std::sync::Arc;
use tokio::fs::{File, ReadDir};
use tokio::io::AsyncReadExt;

pub async fn cycle_monitor() {
    unimplemented!()
}

const BACKUP_PATH: &str = "backup";

pub struct ReUploader {
    backup: ReadDir,
    icda: Arc<ICDA>,
}

impl ReUploader {
    pub async fn new(icda: ICDA) -> Self {
        // create backup dir
        tokio::fs::create_dir(BACKUP_PATH)
            .await
            .expect("failed to create backup dir");

        let backup = tokio::fs::read_dir(BACKUP_PATH)
            .await
            .expect("failed to read backup dir");

        let icda = Arc::new(icda);

        Self { backup, icda }
    }

    // cycling monitor backup file and re-upload the failed chunks
    pub async fn monitor(&mut self) {
        loop {
            match self.backup.next_entry().await {
                Ok(entry) => match entry {
                    Some(entry) => {
                        tracing::warn!("ICDA ReUploader: monitor: catch file {:?}", entry.path());
                        let path = entry.path();
                        let fut = ReUploader::reupload(self.icda.clone(), path);
                        tokio::spawn(fut);
                    }
                    None => {
                        tracing::info!("ICDA ReUploader: monitor: no files to reupload");
                    }
                },
                Err(e) => {
                    tracing::error!(
                        "ICDA ReUploader: monitor: failed to read backup dir: {:?}",
                        e
                    );
                }
            }
            tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
        }
    }

    //todo: 可以不用每次都传path, 以及将这个模块放到icda里面
    // 以及将对chunk的deserialize以及命名放到这个模块
    async fn reupload(icda: Arc<ICDA>, path: PathBuf) {
        let mut file = File::open(&path).await.expect("failed to open file");
        let mut buffer = Vec::new();
        file.read_to_end(&mut buffer)
            .await
            .expect("failed to read file");

        // file's name is : chunk-{canister_id}-{chunk.index}.bin
        let canister_id = parse_canister_id_from_file_name(path);
        let sc = icda
            .storage_canisters_map
            .get(&canister_id)
            .expect("failed to get canister")
            .clone();

        let chunk: BlobChunk = bincode::deserialize(&buffer).expect("failed to deserialize");

        loop {
            match sc.save_blob(&chunk).await {
                Ok(_) => {
                    tracing::info!(
                        "ICDA ReUploader: reupload success, canister id: {}, chunk index: {}",
                        canister_id.to_text(),
                        chunk.index
                    );
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        "ICDA ReUploader: reupload failed: canister id: {}, chunk index: {}.  error: {:?}, retry after 60s",
                        canister_id.to_text(),
                        chunk.index,
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }
            }
        }
    }
}

fn parse_canister_id_from_file_name(path: PathBuf) -> Principal {
    let re = Regex::new(r"chunk_(\d+)_\d+.bin").expect("failed to compile regex");
    let canister_id = re
        .captures(
            path.file_name()
                .expect("failed to get file name")
                .to_str()
                .unwrap(),
        )
        .expect("failed to get canister id")
        .get(1)
        .expect("failed to get canister id")
        .as_str();
    Principal::from_text(canister_id).expect("failed to parse canister id")
}
