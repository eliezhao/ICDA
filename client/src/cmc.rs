use std::str::FromStr;

use anyhow::bail;
use candid::{Decode, Encode, Principal};
use cycles_minting_canister::{NotifyCreateCanister, NotifyError};
use ic_agent::Agent;
use ic_types::CanisterId;

use crate::CMC;

#[derive(Clone)]
pub struct CmcAgent {
    pub agent: Agent,
    pub cmc: Principal,
}

impl CmcAgent {
    pub fn new(a: Agent) -> Self {
        Self {
            agent: a,
            cmc: Principal::from_str(CMC).unwrap(),
        }
    }

    pub async fn notify_create_canister(
        &self,
        arg: NotifyCreateCanister,
    ) -> anyhow::Result<CanisterId> {
        println!("begin notify create canister");
        let ic_res = self
            .agent
            .update(&self.cmc, "notify_create_canister")
            .with_arg(Encode!(&arg)?)
            .call_and_wait()
            .await?;

        let res = Decode!(&ic_res, Result<CanisterId, NotifyError>)?;
        println!("Notify CMC Result: {:?}", res);
        match res {
            Ok(cid) => Ok(cid),
            Err(e) => {
                bail!("{}", e)
            }
        }
    }
}
