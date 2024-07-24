use candid::Principal;
use ic_agent::agent::http_transport::reqwest_transport::reqwest::Client;
use ic_agent::agent::http_transport::route_provider::RoundRobinRouteProvider;
use ic_agent::agent::http_transport::ReqwestTransport;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use std::sync::Arc;

const BOUNDARY_NODE_POOL: [&str; 3] = [
    "https://ic0.app",
    "https://162.247.129.233",
    "https://216.52.51.137",
];

#[derive(Clone)]
pub struct RoundRobinAgent {
    agent: Agent,
}

impl RoundRobinAgent {
    pub fn new(identity: BasicIdentity) -> Self {
        let client = Client::builder()
            .use_rustls_tls()
            .build()
            .expect("Could not create HTTP client.");

        let rr_router = Arc::new(
            RoundRobinRouteProvider::new(
                BOUNDARY_NODE_POOL.iter().map(|s| s.to_string()).collect(),
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
