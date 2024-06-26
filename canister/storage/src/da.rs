use candid::{CandidType, Principal};
use serde::{Deserialize, Serialize};

#[derive(Deserialize, Serialize, CandidType, Clone)]
pub struct Config {
    pub owner: Principal, // who can upload to da canister
    pub signature_canister: Principal,
    pub query_response_size: usize,
}

impl Default for Config {
    fn default() -> Self {
        Self {
            signature_canister: Principal::from_text("v3y75-6iaaa-aaaak-qikaa-cai").unwrap(),
            query_response_size: 2621440, // 2.5 * 1024 * 1024
            owner: Principal::from_text(
                "ytoqu-ey42w-sb2ul-m7xgn-oc7xo-i4btp-kuxjc-b6pt4-dwdzu-kfqs4-nae",
            )
            .unwrap(),
        }
    }
}
