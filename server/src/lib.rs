pub mod canister_service;
pub mod storage;
pub mod disperser {
    #![allow(clippy::all)]
    tonic::include_proto!("disperser");
}
