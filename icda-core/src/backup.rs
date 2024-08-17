use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{SystemTime, UNIX_EPOCH};

use candid::Principal;
use regex::Regex;
use serde::{Deserialize, Serialize};
use tokio::fs::{File, ReadDir};
use tokio::io::AsyncReadExt;
use tokio::select;

use crate::icda::ICDA;

pub async fn cycle_monitor() {
    unimplemented!()
}

pub const BACKUP_PATH: &str = "backup";

pub struct ReUploader {
    backup: ReadDir,
    icda: Arc<ICDA>,
}

impl ReUploader {
    pub async fn new(icda: ICDA) -> Self {
        // check if backup file exist
        if !std::path::Path::new(BACKUP_PATH).exists() {
            tokio::fs::create_dir(BACKUP_PATH)
                .await
                .expect("failed to create backup dir");
        }

        let backup = tokio::fs::read_dir(BACKUP_PATH)
            .await
            .expect("failed to read backup dir");

        let icda = Arc::new(icda);

        Self { backup, icda }
    }

    pub async fn start_uploader(self) {
        let backup_thread = async move {
            self.uploader().await;
        };

        let ctrl_c = tokio::signal::ctrl_c();
        select! {
                _ = backup_thread => {},
                _ = ctrl_c => {
                    tracing::info!("ICDA ReUploader: monitor: ctrl-c received, shutdown");
            }
        }
    }

    // cycling monitor backup file and re-upload the failed chunks
    async fn uploader(mut self) {
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
            tokio::time::sleep(tokio::time::Duration::from_secs(600)).await;
        }
    }

    async fn reupload(icda: Arc<ICDA>, path: PathBuf) {
        let mut buffer = Vec::new();

        {
            let mut file = File::open(&path).await.expect("failed to open file");
            file.read_to_end(&mut buffer)
                .await
                .expect("failed to read file");
        }

        // file's name is : chunk-{canister_id}-{chunk.index}.bin
        let canister_id = Self::parse_canister_id_from_file_name(&path);
        let sc = icda
            .storage_canisters_map
            .get(&canister_id)
            .expect("failed to get canister")
            .clone();

        let serialized_chunk: Vec<u8> =
            bincode::deserialize(&buffer).expect("failed to deserialize");
        drop(buffer);

        loop {
            // 这里因为是reupload，所以暂时不考虑会有很大量的chunk的累计，所以直接用了clone
            match sc.save_blob(serialized_chunk.clone()).await {
                Ok(_) => {
                    tracing::info!(
                        "ICDA ReUploader: reupload success, canister id: {}",
                        canister_id.to_text(),
                    );
                    // remove file
                    tokio::fs::remove_file(path)
                        .await
                        .expect("failed to remove file");
                    break;
                }
                Err(e) => {
                    tracing::error!(
                        "ICDA ReUploader: reupload failed: canister id: {}.  error: {:?}, retry after 60s",
                        canister_id.to_text(),
                        e
                    );
                    tokio::time::sleep(tokio::time::Duration::from_secs(60)).await;
                }
            }
        }
    }
}

impl ReUploader {
    pub async fn save<'a, T>(data: &'a T, file_name: String)
    where
        T: Serialize + Deserialize<'a>,
    {
        // save chunks into local storage
        let serialized = bincode::serialize(&data).unwrap();

        // 放到icda 的 reuploader中
        let _ = tokio::fs::write(format!("{BACKUP_PATH}/{}", file_name,), serialized).await;
    }

    // (canister_id)_chunk_(system_time).bin
    pub fn generate_backup_file_name(canister_id: String, data_type: &str) -> String {
        canister_id
            + "_"
            + data_type
            + "_"
            + &SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .expect("Failed to get timestamp")
                .as_nanos()
                .to_string()
            + ".bin"
    }

    // (canister_id)_chunk_(system_time).bin
    fn parse_canister_id_from_file_name(path: &Path) -> Principal {
        let re = Regex::new(r"([a-z0-9-]+)_chunk_\d+\.bin").expect("failed to compile regex");
        let file_name = path
            .file_name()
            .expect("failed to get file name")
            .to_str()
            .expect("failed to convert to str");
        let captures = re.captures(file_name).expect("failed to match regex");
        let canister_id = captures.get(1).expect("failed to get canister id").as_str();
        Principal::from_text(canister_id).expect("failed to parse canister id")
    }
}

#[cfg(test)]
mod test {
    use super::*;

    #[test]
    fn test_generate_backup_file_name() {
        let canister_id = "r2xtu-uiaaa-aaaag-alf6q-cai".to_string();
        let data_type = "chunk";
        let file_name = ReUploader::generate_backup_file_name(canister_id, data_type);
        let re = Regex::new(r"([a-z0-9-]+)_chunk_\d+\.bin").expect("failed to compile regex");
        assert!(re.is_match(&file_name));
    }

    #[test]
    fn test_parse_canister_id_from_file_name() {
        let canister_id = "r2xtu-uiaaa-aaaag-alf6q-cai".to_string();
        let data_type = "chunk";
        let file_name = ReUploader::generate_backup_file_name(canister_id.clone(), data_type);
        let path = PathBuf::from(file_name);
        let parsed_canister_id = ReUploader::parse_canister_id_from_file_name(&path);
        assert_eq!(canister_id, parsed_canister_id.to_text());
    }
}
