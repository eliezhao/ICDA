use candid::Principal;
use ic_agent::agent::http_transport::reqwest_transport::reqwest::Client;
use ic_agent::agent::http_transport::route_provider::RoundRobinRouteProvider;
use ic_agent::agent::http_transport::ReqwestTransport;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use std::sync::Arc;

const BOUNDARY_NODE_POOL: [&str; 15] = [
    "63.251.162.12",
    "147.75.202.74",
    "162.247.129.233",
    "193.118.59.140",
    "193.118.63.169",
    "193.118.63.173",
    "193.118.63.170",
    "212.71.124.187",
    "212.71.124.188",
    "212.71.124.189",
    "212.71.124.190",
    "212.71.124.187",
    "216.52.51.137",
    "216.52.51.138",
    "216.52.51.139",
];

#[derive(Clone)]
pub struct RoundRobinAgent {
    agent: Agent,
}

impl RoundRobinAgent {
    pub fn new(identity: BasicIdentity) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .danger_accept_invalid_certs(true)
            .build()
            .expect("Could not create HTTP client.");

        let rr_router = Arc::new(
            RoundRobinRouteProvider::new(
                BOUNDARY_NODE_POOL
                    .iter()
                    .map(|s| format!("{}{}", "https://", s))
                    .collect(),
            )
            .unwrap(),
        );

        let transport = ReqwestTransport::create_with_client_route(rr_router, client).unwrap();

        let agent = Agent::builder()
            .with_identity(identity)
            .with_transport(transport)
            .build()
            .unwrap();

        Self { agent }
    }

    pub fn get_principal(&self) -> Result<Principal, String> {
        self.agent.get_principal()
    }

    pub async fn update_call(
        &self,
        canister_id: &Principal,
        function_name: &str,
        args: Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        let raw = self
            .agent
            .update(canister_id, function_name)
            .with_arg(args)
            .call_and_wait()
            .await?;
        Ok(raw)
    }

    pub async fn query_call(
        &self,
        canister_id: &Principal,
        function_name: &str,
        args: Vec<u8>,
    ) -> anyhow::Result<Vec<u8>> {
        let res = self
            .agent
            .query(canister_id, function_name)
            .with_arg(args)
            .call()
            .await?;

        Ok(res)
    }
}
