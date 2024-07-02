use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

const SIGNATURE_CANISTER: &str = "r34pn-oaaaa-aaaak-qinga-cai";
const QUERY_RESPONSE_SIZE: usize = 2621440; // 2.5 * 1024 * 1024 = 2.5 MB
const OWNER: &str = "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae";

#[derive(Deserialize, Serialize, CandidType, Clone)]
pub struct Config {
    pub owner: Principal, // who can upload to da canister
    pub signature_canister: Principal,
    pub query_response_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            signature_canister: Principal::from_text(SIGNATURE_CANISTER).unwrap(),
            query_response_size: QUERY_RESPONSE_SIZE,
            owner: Principal::from_text(OWNER).unwrap(),
        }
    }
}
