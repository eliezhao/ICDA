use crate::canister_interface::rr_agent::RoundRobinAgent;
use crate::canister_interface::signature::{ConfirmationStatus, SignatureCanister};
use crate::canister_interface::storage::{BlobChunk, RoutingInfo, StorageCanister};
use anyhow::bail;
use anyhow::Result;
use candid::{Deserialize, Principal};
use ic_agent::identity::BasicIdentity;
use rand::random;
use serde::Serialize;
use sha2::Digest;
use std::collections::HashMap;
use std::fmt::{Debug, Formatter};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};
use tokio::sync::Mutex;
use tracing::{error, info, warn};

pub const REPLICA_NUM: usize = 1;
pub const COLLECTION_SIZE: usize = 11;
// 1 week in nanos
pub const BLOB_LIVE_TIME: u128 = 7 * 24 * 60 * 60 * 1_000_000_000;
pub const CONFIRMATION_BATCH_SIZE: usize = 12;
pub const CONFIRMATION_LIVE_TIME: u32 = 60 * 60 * 24 * 7 + 1; // 1 week in nanos
pub const QUERY_RESPONSE_SIZE: usize = 2621440; // 2.5 * 1024 * 1024 = 2.5 MB
pub const CANISTER_THRESHOLD: u32 = 30240;
pub const SIGNATURE_CANISTER: &str = "r34pn-oaaaa-aaaak-qinga-cai";
pub(crate) const DEFAULT_OWNER: &str =
    "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae";

// canister collections
pub const CANISTER_COLLECTIONS: [[&str; REPLICA_NUM]; COLLECTION_SIZE] = [
    ["hxctj-oiaaa-aaaap-qhltq-cai"], // nl6hn-ja4yw-wvmpy-3z2jx-ymc34-pisx3-3cp5z-3oj4a-qzzny-jbsv3-4qe
    ["v3y75-6iaaa-aaaak-qikaa-cai"], // opn46-zyspe-hhmyp-4zu6u-7sbrh-dok77-m7dch-im62f-vyimr-a3n2c-4ae
    ["nnw5b-eqaaa-aaaak-qiqaq-cai"], // opn46-zyspe-hhmyp-4zu6u-7sbrh-dok77-m7dch-im62f-vyimr-a3n2c-4ae
    ["wcrzb-2qaaa-aaaap-qhpgq-cai"], // nl6hn-ja4yw-wvmpy-3z2jx-ymc34-pisx3-3cp5z-3oj4a-qzzny-jbsv3-4qe
    ["y446g-jiaaa-aaaap-ahpja-cai"], // 3hhby-wmtmw-umt4t-7ieyg-bbiig-xiylg-sblrt-voxgt-bqckd-a75bf-rqe
    ["hmqa7-byaaa-aaaam-ac4aq-cai"], // 4ecnw-byqwz-dtgss-ua2mh-pfvs7-c3lct-gtf4e-hnu75-j7eek-iifqm-sqe
    ["jeizw-6yaaa-aaaal-ajora-cai"], // 6pbhf-qzpdk-kuqbr-pklfa-5ehhf-jfjps-zsj6q-57nrl-kzhpd-mu7hc-vae
    ["vrk5x-dyaaa-aaaan-qmrsq-cai"], // cv73p-6v7zi-u67oy-7jc3h-qspsz-g5lrj-4fn7k-xrax3-thek2-sl46v-jae
    ["zhu6y-liaaa-aaaal-qjlmq-cai"], // e66qm-3cydn-nkf4i-ml4rb-4ro6o-srm5s-x5hwq-hnprz-3meqp-s7vks-5qe
    ["oyfj2-gaaaa-aaaak-akxdq-cai"], // k44fs-gm4pv-afozh-rs7zw-cg32n-u7xov-xqyx3-2pw5q-eucnu-cosd4-uqe
    ["r2xtu-uiaaa-aaaag-alf6q-cai"], // lspz2-jx4pu-k3e7p-znm7j-q4yum-ork6e-6w4q6-pijwq-znehu-4jabe-kqe
];

#[derive(Serialize, Deserialize, Clone, Default)]
pub struct BlobKey {
    pub digest: [u8; 32],
    pub expiry_timestamp: u128, // current system time + live time
    pub routing_info: RoutingInfo,
}

impl Debug for BlobKey {
    fn fmt(&self, f: &mut Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("BlobKey")
            .field("digest", &hex::encode(self.digest))
            .field("expiry_timestamp", &self.expiry_timestamp)
            .field("routing_info", &self.routing_info)
            .finish()
    }
}

#[derive(Clone)]
pub struct ICDA {
    canister_collection_index: Arc<Mutex<usize>>,
    pub storage_canisters_map: HashMap<Principal, StorageCanister>,
    pub signature_canister: SignatureCanister,
}

impl ICDA {
    pub async fn new(pem_path: String) -> Result<Self> {
        let identity = BasicIdentity::from_pem_file(pem_path)?;

        let agent = Arc::new(RoundRobinAgent::new(identity));

        let mut storage_canisters_map = HashMap::new();
        for storage_cid in CANISTER_COLLECTIONS.iter().flat_map(|x| x.iter()) {
            let sc =
                StorageCanister::new(Principal::from_text(storage_cid).unwrap(), agent.clone());
            storage_canisters_map.insert(Principal::from_text(storage_cid).unwrap(), sc);
        }

        let signature_canister = SignatureCanister::new(
            Principal::from_text(SIGNATURE_CANISTER).unwrap(),
            agent.clone(),
        );

        loop {
            if let Ok(res) = signature_canister.public_key().await {
                if !res.is_empty() {
                    break;
                } else {
                    match signature_canister.init().await {
                        Ok(_) => {
                            break;
                        }
                        Err(e) => {
                            warn!("ICDA::new(): signature canister init failed, error: {:?}, retry after 5 seconds",e);
                            tokio::time::sleep(Duration::from_secs(5)).await;
                        }
                    }
                }
            }
        }

        let canister_collection_index = Arc::new(Mutex::new(random::<usize>() % COLLECTION_SIZE));

        // create backup dir
        let _ = tokio::fs::create_dir("backup").await;

        Ok(Self {
            canister_collection_index,
            storage_canisters_map,
            signature_canister,
        })
    }

    pub async fn push_blob_to_canisters(&self, blob: Vec<u8>) -> Result<BlobKey> {
        let blob_digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        let timestamp = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .expect("Failed to get timestamp")
            .as_nanos();
        let total_size = blob.len();
        let storage_canisters = self.get_storage_canisters().await;
        let routing_canisters = storage_canisters
            .iter()
            .map(|sc| sc.canister_id)
            .collect::<Vec<_>>();

        let fut = async move {
            let blob_chunks = Arc::new(BlobChunk::generate_chunks(blob, blob_digest, timestamp));

            for sc in storage_canisters {
                let _chunks = blob_chunks.clone();
                let fut = async move {
                    let cid = sc.canister_id;
                    let hexed_digest = hex::encode(blob_digest);
                    match Self::push_chunks_to_canister(sc, _chunks).await {
                        Ok(_) => {
                            info!(
                                "ICDA::save_blob_chunk(): cid = {}, digest: {}, success",
                                cid.to_text(),
                                hexed_digest
                            );
                        }
                        Err(e) => {
                            error!(
                                "ICDA::save_blob_chunk(): cid = {}, digest: {}, error: {:?}",
                                cid.to_text(),
                                hexed_digest,
                                e
                            );
                        }
                    }
                };
                tokio::spawn(fut);
            }
        };
        tokio::spawn(fut);

        let blob_key = BlobKey {
            digest: blob_digest,
            expiry_timestamp: timestamp + BLOB_LIVE_TIME,
            routing_info: RoutingInfo {
                total_size,
                host_canisters: routing_canisters,
            },
        };

        Ok(blob_key)
    }

    pub async fn get_blob_from_canisters(&self, blob_key: BlobKey) -> Result<Vec<u8>> {
        let blob_key = Arc::new(blob_key);

        // inspect expiry timestamp
        let current_timestamp = SystemTime::now().duration_since(UNIX_EPOCH)?.as_nanos();

        if blob_key.expiry_timestamp < current_timestamp {
            bail!(
                "ICDA::get_blob(): expired: key.expiry_timestamp = {:?}, current_timestamp = {:?}",
                blob_key.expiry_timestamp,
                current_timestamp
            );
        }

        let storage_canisters = blob_key
            .routing_info
            .host_canisters
            .iter()
            .map(|cid| {
                self.storage_canisters_map
                    .get(cid)
                    .expect("Failed to get storage canister")
                    .clone()
            })
            .collect::<Vec<_>>();

        // get from canisters
        let (tx, mut rx) = tokio::sync::mpsc::channel(REPLICA_NUM);
        for sc in storage_canisters {
            let _tx = tx.clone();
            let _key = blob_key.clone();
            let fut = async move {
                let cid = sc.canister_id;
                let res = Self::get_blob_from_canister(sc, _key).await;
                match res {
                    Ok(blob) => {
                        let _ = _tx.send(blob).await;
                    }
                    Err(e) => {
                        error!("ICDA::get_blob(): cid: {}, error: {:?}", cid.to_text(), e);
                    }
                }
                drop(_tx);
            };
            tokio::spawn(fut);
        }

        loop {
            tokio::select! {
                msg = rx.recv() => {
                    match msg {
                        Some(blob) => {
                            return Ok(blob);
                        },
                        None => {
                            // No more senders and no message received
                            error!("All senders are closed and no more messages.");
                            break;
                        }
                    }
                },
                _ = tokio::time::sleep(Duration::from_secs(1)) => {
                    if rx.is_closed() {
                        error!("ICDA: get_blob_from_canisters: all senders are closed");
                        break;
                    }
                }
            }
        }

        bail!("ICDA::get_blob(): failed to get blob")
    }

    pub async fn get_blob_confirmation(
        sc: &SignatureCanister,
        digest: [u8; 32],
    ) -> Result<ConfirmationStatus> {
        match sc.get_confirmation(digest).await {
            Ok(confirmation) => Ok(confirmation),
            Err(e) => {
                bail!(
                    "ICDA::get_confirmation(): failed to get confirmation, error: {}",
                    e
                );
            }
        }
    }
}

impl ICDA {
    // push chunks to a single canister
    async fn push_chunks_to_canister(
        sc: StorageCanister,
        chunks: Arc<Vec<BlobChunk>>,
    ) -> Result<()> {
        for chunk in chunks.iter() {
            // simple re-upload
            for i in 0..3 {
                if let Err(e) = sc.save_blob(chunk).await {
                    warn!(
                        "ICDA::save_blob_chunk(): cid: {}, error: {:?}, retry after 5 seconds",
                        sc.canister_id.to_text(),
                        e
                    );
                    if i == 2 {
                        // save chunks into local storage
                        let serialized = bincode::serialize(&chunk).unwrap();
                        let _ = tokio::fs::write(
                            format!(
                                "backup/chunk_{}_{}.bin",
                                sc.canister_id.to_text(),
                                chunk.index
                            ),
                            serialized,
                        )
                        .await;

                        warn!(
                            "ICDA::save_blob_chunk(): cid: {}, error: {:?}, retry 3 times failed. Save chunk to local storage: chunk_{}_{}.bin",
                            sc.canister_id.to_text(),
                            e,
                            sc.canister_id.to_text(),
                            chunk.index
                        );

                        bail!(
                            "ICDA::save_blob_chunk(): cid: {}, digest: {}, error: {:?}, retry 3 times failed",
                            sc.canister_id.to_text(),
                            hex::encode(chunk.digest),
                            e
                        );
                    }
                    tokio::time::sleep(Duration::from_secs(5)).await;
                } else {
                    break;
                }
            }
        }

        Ok(())
    }

    // get blob from canister and check digest
    async fn get_blob_from_canister(sc: StorageCanister, key: Arc<BlobKey>) -> Result<Vec<u8>> {
        // 创建一样大小的buffer
        let mut blob = Vec::with_capacity(key.routing_info.total_size);

        let mut slice = sc.get_blob(key.digest).await?;
        blob.extend(slice.data);

        while let Some(next_index) = slice.next {
            // get blob by index
            slice = sc.get_blob_with_index(key.digest, next_index).await?;
            blob.extend(slice.data);
        }

        if blob.is_empty() {
            bail!(
                "ICDA::get_blob_from_canisters(): failed to get blob from canisters, blob is empty"
            );
        }

        let digest: [u8; 32] = sha2::Sha256::digest(&blob).into();
        if !key.digest.eq(&digest) {
            bail!("ICDA::get_blob_from_canisters(): blob digest not match");
        }

        Ok(blob)
    }

    // get storage canisters in the current round
    async fn get_storage_canisters(&self) -> Vec<StorageCanister> {
        let cids;
        {
            let mut index = self.canister_collection_index.lock().await;
            cids = CANISTER_COLLECTIONS.get(*index).unwrap();

            *index += 1;
            *index %= COLLECTION_SIZE;
        }

        let storage_canisters = cids
            .iter()
            .map(|cid| {
                self.storage_canisters_map
                    .get(&Principal::from_text(cid).unwrap())
                    .expect("Failed to get storage canister")
                    .clone()
            })
            .collect::<Vec<_>>();
        info!("ICDA::get_storage_canisters(): {:?}", cids);

        storage_canisters
    }
}

#[tokio::test]
async fn test_icda() {
    let icda = ICDA::new("../identity/identity.pem".to_string())
        .await
        .unwrap();

    let blob = vec![1, 2, 3, 4, 5, 6, 7, 8, 9, 0, 15];

    let before = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Failed to get timestamp")
        .as_nanos();
    let blob_key = icda.push_blob_to_canisters(blob.clone()).await.unwrap();
    let after = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .expect("Failed to get timestamp")
        .as_nanos();
    println!("before: {}, after: {}", before, after);

    let blob2 = icda.get_blob_from_canisters(blob_key).await.unwrap();

    assert_eq!(blob, blob2);
}
