//! Eigen DA gRPC server(mock) implementation.
//!
//! Cargo run -p da_server -- s3 --profile profile-abc --bucket bucket-xyz
//! cargo run -p da_server -- local --db-path /tmp/da_server.db

use anyhow::Result;
use async_trait::async_trait;
use clap::{ArgAction, Parser, Subcommand};
use governor::{Quota, RateLimiter};
use server::disperser::disperser_server::{Disperser, DisperserServer};
use server::disperser::{
    AuthenticatedReply, AuthenticatedRequest, BatchHeader, BatchMetadata, BlobHeader, BlobInfo,
    BlobStatus, BlobStatusReply, BlobStatusRequest, BlobVerificationProof, DisperseBlobReply,
    DisperseBlobRequest, G1Commitment, RetrieveBlobReply, RetrieveBlobRequest,
};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::num::NonZeroU32;
use std::path::PathBuf;
use std::pin::Pin;
use std::sync::Arc;
use std::time::Duration;

use icda_core::icda::ICDA;
use server::storage::{LocalStorage, S3Storage, Storage};
use tonic::transport::Server;

#[derive(Debug, Parser)]
struct Cli {
    /// gRPC port.
    #[arg(long, global = true, default_value_t = 5005, help = "gRPC server port")]
    server_port: u16,

    /// Disable rate limiting, rate limiting is enabled by default.
    #[arg(
        long,
        global = true,
        action(ArgAction::SetTrue),
        help = "Disable rate limiting"
    )]
    disable_rate_limiting: bool,

    /// Rate limit.
    #[arg(
        long,
        global = true,
        default_value_t = 2_000_000,
        help = "Rate limit (bytes per second)"
    )]
    bytes_per_second: u32,

    /// Max burst to allow.
    #[arg(
        long,
        global = true,
        default_value_t = 2097152, // 2 * (1 << 20),
        help = "Max burst to allow with rate limiting, in bytes"
    )]
    max_burst: u32,

    #[clap(subcommand)]
    cmd: Cmd,
}

#[derive(Debug, Subcommand)]
enum Cmd {
    S3(S3),
    Local(Local),
    IC(IC),
}

/// Params for using S3 for persistence.
#[derive(Debug, Parser)]
pub struct S3 {
    /// Region for the S3 bucket.
    #[structopt(short, long, required(true))]
    profile: String,

    /// S3 bucket.
    #[structopt(short, long, required(true))]
    bucket: String,
}

#[derive(Debug, Parser)]
pub struct IC {
    #[arg(long)]
    pem_path: String,
}

/// Params for using local database for persistence.
#[derive(Debug, Parser)]
pub struct Local {
    #[arg(long)]
    db_path: PathBuf,
}

/// gRPC server implementation.
struct DAServer {
    storage: Box<dyn Storage>,
    flow_rate: Option<Arc<governor::DefaultDirectRateLimiter>>,
}

#[async_trait]
impl Disperser for DAServer {
    async fn disperse_blob(
        &self,
        request: tonic::Request<DisperseBlobRequest>,
    ) -> std::result::Result<tonic::Response<DisperseBlobReply>, tonic::Status> {
        let data = request.into_inner().data;
        if let Some(flow_rate) = &self.flow_rate {
            if let Some(len) = NonZeroU32::new(data.len() as u32) {
                flow_rate.until_n_ready(len).await.unwrap();
            }
        }

        self.storage
            .save_blob(data)
            .await
            .map(|blob_id| {
                tonic::Response::new(DisperseBlobReply {
                    result: BlobStatus::Confirmed.into(),
                    request_id: blob_id,
                })
            })
            .map_err(|err| tonic::Status::internal(format!("disperse_blob: {err:?}")))
    }
    type DisperseBlobAuthenticatedStream =
        Pin<Box<dyn tokio_stream::Stream<Item = Result<AuthenticatedReply, tonic::Status>> + Send>>;

    async fn disperse_blob_authenticated(
        &self,
        _request: tonic::Request<tonic::Streaming<AuthenticatedRequest>>,
    ) -> std::result::Result<tonic::Response<Self::DisperseBlobAuthenticatedStream>, tonic::Status>
    {
        unimplemented!()
    }

    async fn get_blob_status(
        &self,
        request: tonic::Request<BlobStatusRequest>,
    ) -> std::result::Result<tonic::Response<BlobStatusReply>, tonic::Status> {
        let blob_id = request.into_inner().request_id;
        let blob = self
            .storage
            .get_blob(blob_id.clone())
            .await
            .map_err(|err| tonic::Status::internal(format!("get_blob_status: {err:?}")))?;

        // The blob_id is stored in batch_header_hash in serialized format,
        // block_index is ignored.
        let hdr = BlobHeader {
            commitment: Some(G1Commitment {
                x: vec![0; 32],
                y: vec![0; 32],
            }),
            data_length: blob.len() as u32,
            ..Default::default()
        };
        let metadata = BatchMetadata {
            batch_header: Some(BatchHeader {
                batch_root: vec![0; 32],
                ..Default::default()
            }),
            signatory_record_hash: vec![0; 32],
            fee: vec![0; 16],
            batch_header_hash: blob_id,
            ..Default::default()
        };
        let verification_proof = BlobVerificationProof {
            batch_metadata: Some(metadata),
            ..Default::default()
        };

        Ok(tonic::Response::new(BlobStatusReply {
            status: BlobStatus::Confirmed.into(),
            info: Some(BlobInfo {
                blob_header: Some(hdr),
                blob_verification_proof: Some(verification_proof),
            }),
        }))
    }

    async fn retrieve_blob(
        &self,
        request: tonic::Request<RetrieveBlobRequest>,
    ) -> std::result::Result<tonic::Response<RetrieveBlobReply>, tonic::Status> {
        let blob_id = request.into_inner().batch_header_hash;
        self.storage
            .get_blob(blob_id)
            .await
            .map(|blob| tonic::Response::new(RetrieveBlobReply { data: blob }))
            .map_err(|err| tonic::Status::internal(format!("retrieve_blob: {err:?}")))
    }
}

#[tokio::main]
async fn main() -> Result<()> {
    let (writer, _guard) = tracing_appender::non_blocking(std::io::stdout());
    tracing_subscriber::fmt()
        .with_writer(writer)
        .with_line_number(true)
        .with_env_filter(tracing_subscriber::EnvFilter::from_default_env())
        .init();

    let Cli {
        server_port,
        disable_rate_limiting,
        bytes_per_second,
        max_burst,
        cmd,
    } = <Cli as clap::Parser>::parse();

    let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::UNSPECIFIED), server_port);

    let mut is_icda = false;
    let storage: Box<dyn Storage> = match cmd {
        Cmd::S3(S3 { profile, bucket }) => Box::new(S3Storage::new(profile, bucket).await),
        Cmd::Local(Local { db_path }) => Box::new(LocalStorage::new(db_path)?),
        Cmd::IC(IC { pem_path }) => {
            is_icda = true;
            Box::new(ICDA::new(pem_path).await?)
        }
    };

    // Save/restore a test blob to ensure the backend is set up.
    let id = storage
        .save_blob(vec![102, 97, 105, 122, 32, 104, 97, 110, 97, 10])
        .await
        .unwrap();

    tracing::info!("checking service...");

    // check if cmd is IC
    if is_icda {
        tokio::time::sleep(Duration::from_secs(10)).await
    }

    let _ret = storage.get_blob(id).await.unwrap();
    tracing::trace!("Initial save/restore successful");

    let flow_rate = if !disable_rate_limiting {
        tracing::info!(
            "Rate limiting enabled: bytes_per_second = {bytes_per_second}, max_burst = {max_burst}"
        );
        let burst = NonZeroU32::new(max_burst).expect("Burst should be non-zero");
        let bytes_per_min =
            NonZeroU32::new(60 * bytes_per_second).expect("Bytes per second should be non-zero");
        let quota = Quota::per_minute(bytes_per_min).allow_burst(burst);
        Some(Arc::new(RateLimiter::direct(quota)))
    } else {
        tracing::info!("Rate limiting disabled");
        None
    };

    let server = DAServer { storage, flow_rate };
    Server::builder()
        .add_service(DisperserServer::new(server))
        .serve(addr)
        .await?;

    Ok(())
}
