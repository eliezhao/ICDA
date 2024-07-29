use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};
use std::collections::HashSet;

const SIGNATURE_CANISTER: &str = "r34pn-oaaaa-aaaak-qinga-cai";
const OWNER: &str = "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae";
const TEST_IDENTITY: &str = "rtw64-dzklf-dqtzm-lhev7-ufjji-fnmfq-bkyyf-ljaod-ldfpb-w2zyk-7ae";
const QUERY_RESPONSE_SIZE: usize = 2621440; // 2.5 * 1024 * 1024 = 2.5 MB
const CANISTER_THRESHOLD: u32 = 30240;
const CHUNK_SIZE: usize = 1 << 20; // 1M

#[derive(Deserialize, Serialize, CandidType, Clone)]
pub struct Config {
    pub owner: HashSet<Principal>, // who can upload to da canister
    pub signature_canister: Principal,
    pub chunk_size: usize,
    pub query_response_size: usize,
    pub canister_storage_threshold: u32,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            signature_canister: Principal::from_text(SIGNATURE_CANISTER).unwrap(),
            chunk_size: CHUNK_SIZE,
            query_response_size: QUERY_RESPONSE_SIZE,
            owner: HashSet::from_iter(vec![
                Principal::from_text(OWNER).unwrap(),
                Principal::from_text(TEST_IDENTITY).unwrap(),
            ]),
            canister_storage_threshold: CANISTER_THRESHOLD,
        }
    }
}
