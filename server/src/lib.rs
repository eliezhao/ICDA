pub mod canister_interface;
pub mod icda;
pub mod storage;
pub mod disperser {
    #![allow(clippy::all)]
    tonic::include_proto!("disperser");
}
