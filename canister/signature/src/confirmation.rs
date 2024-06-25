/*
 ******************************************
 *                                        *
 *          Confirmation Types             *
 *                                        *
 ******************************************
*/

use std::borrow::Cow;
use std::str::FromStr;

use candid::{CandidType, Decode, Deserialize, Encode};
use ic_stable_structures::storable::Bound;
use ic_stable_structures::Storable;
use serde::Serialize;

use crate::CONFIRMATION_CONFIG;

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct Confirmation {
    pub root: [u8; 32],       // merkle root hash
    pub proof: Vec<[u8; 32]>, // merkle proof
    pub signature: String,    // hex encoded signature
}

#[derive(CandidType, Deserialize, Serialize, Clone)]
pub struct BatchConfirmation {
    pub signature: String,
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
        max_size: 500, // 500 bytes > 实际使用(64 bytes signature + 12 * 32 bytes nodes)
        is_fixed_size: false,
    };
}

impl Default for BatchConfirmation {
    fn default() -> Self {
        Self {
            signature: "".to_string(),
            root: [0x00u8; 32],
            nodes: Vec::with_capacity(
                CONFIRMATION_CONFIG.with_borrow(|s| s.confirmation_batch_size) as usize,
            ),
        }
    }
}

#[derive(CandidType, Serialize, Deserialize, Debug)]
pub struct ConfirmationConfig {
    pub confirmation_batch_size: u32,
    pub confirmation_live_time: u32,
}

impl Default for ConfirmationConfig {
    fn default() -> Self {
        Self {
            confirmation_live_time: 60 * 60 * 24 * 7 + 1, // 7 days
            confirmation_batch_size: 12,                  // 12 blobs per confirmation
        }
    }
}

#[derive(CandidType, Deserialize, Serialize, Clone, Default)]
pub(crate) struct BatchInfo {
    pub batch_index: u32,
    pub leaf_index: usize,
}

impl Storable for BatchInfo {
    fn to_bytes(&self) -> Cow<[u8]> {
        Cow::Owned(Encode!(self).unwrap())
    }

    fn from_bytes(bytes: Cow<[u8]>) -> Self {
        Decode!(bytes.as_ref(), Self).unwrap()
    }

    const BOUND: Bound = Bound::Bounded {
        max_size: 8,
        is_fixed_size: true,
    };
}
