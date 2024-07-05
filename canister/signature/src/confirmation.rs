/*
 ******************************************
 *                                        *
 *          Confirmation Types             *
 *                                        *
 ******************************************
*/

use std::borrow::Cow;
use std::collections::HashSet;

use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::Storable;
use serde::Serialize;

const REPLICA_NUM: usize = 2; // 1 blob, 2 canister replicas
const COLLECTION_SIZE: usize = 20; // current subnets number, 20 subnets and 40 canisters

const CONFIRMATION_BATCH_SIZE: usize = 12; // current size of the batch
const CONFIRMATION_LIVE_TIME: u32 = 120961; // 1/12 * 1 week in secs = 12 * 60 * 24 * 7 + 1
const CANISTER_COLLECTIONS: [[&str; REPLICA_NUM]; COLLECTION_SIZE] =
    [["hxctj-oiaaa-aaaap-qhltq-cai", "v3y75-6iaaa-aaaak-qikaa-cai"]; COLLECTION_SIZE];

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub enum ConfirmationStatus {
    Pending,
    Confirmed(Confirmation),
    Invalid,
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Proof {
    pub proof_bytes: Vec<u8>,
    pub leaf_index: usize,
    pub leaf_digest: [u8; 32],
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Confirmation {
    pub root: [u8; 32],    // merkle root hash
    pub proof: Proof,      // merkle proof
    pub signature: String, // hex encoded signature
}

#[derive(CandidType, Deserialize, Serialize, Clone, Debug)]
pub struct BatchConfirmation {
    pub signature: Option<String>,
    pub root: [u8; 32],
    pub nodes: Vec<[u8; 32]>, // 12 个 blob的digest
}

impl Storable for BatchConfirmation {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        // 1024 bytes > 实际使用(64 bytes signature + 12 * 32 bytes nodes) + candid = 530 bytes,
        // encoded => 594
        max_size: 1024,
        is_fixed_size: false,
    };
}

impl Default for BatchConfirmation {
    fn default() -> Self {
        Self {
            signature: None,
            root: [0x00u8; 32],
            nodes: Vec::with_capacity(CONFIRMATION_BATCH_SIZE),
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
pub(crate) struct BatchIndex(pub u32); // u32 => 136 years, 1 block / second

impl Storable for BatchIndex {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 15,
        is_fixed_size: false,
    };
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct Config {
    pub confirmation_batch_size: usize,
    pub confirmation_live_time: u32,
    pub da_canisters: HashSet<Principal>,
    pub owner: Principal, // who can change confirmation config
}

impl Default for Config {
    fn default() -> Self {
        let mut da_canisters = HashSet::with_capacity(COLLECTION_SIZE);
        CANISTER_COLLECTIONS.iter().for_each(|x| {
            x.iter().for_each(|x| {
                da_canisters.insert(Principal::from_text(x).unwrap());
            });
        });

        Self {
            confirmation_live_time: CONFIRMATION_LIVE_TIME, // 7 days in batch number
            confirmation_batch_size: CONFIRMATION_BATCH_SIZE, // 12 blobs per confirmation
            da_canisters,
            owner: Principal::from_text(
                "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae",
            )
            .unwrap(),
        }
    }
}
