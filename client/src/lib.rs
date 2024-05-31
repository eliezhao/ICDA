use anyhow::{bail, Result};
use candid::{Decode, Encode, Nat, Principal};
use cycles_minting_canister::{NotifyCreateCanister, NotifyError};
use ic_agent::Agent;
use ic_types::{CanisterId, PrincipalId, SubnetId};
use icp_ledger::{AccountIdentifier, NotifyCanisterArgs, Subaccount, TransferArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::{TransferArg, TransferError};
use serde::{Deserialize, Serialize};
use std::str::FromStr;

pub const LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const CMC: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";

// todo : 选10个合适的subnet，至少120G可用stable memory
pub const SUBNETS: [&str; 10] =
    ["2fq7c-slacv-26cgz-vzbx2-2jrcs-5edph-i5s2j-tck77-c3rlz-iobzx-mqe"; 10];

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

    pub async fn notify_create_canister(&self, arg: NotifyCreateCanister) -> Result<CanisterId> {
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

#[derive(Clone)]
pub struct LedgerAgent {
    agent: Agent,
    ledger: Principal,
}

impl LedgerAgent {
    pub fn new(agent: Agent) -> Self {
        Self {
            agent,
            ledger: Principal::from_str(LEDGER).unwrap(),
        }
    }

    /// Returns the balance of the account given as argument.
    pub async fn balance_of(&self, account: Account) -> Result<Nat> {
        let res = self
            .agent
            .query(&self.ledger, "icrc1_balance_of")
            .with_arg(Encode!(&account)?)
            .call()
            .await?;

        Ok(Decode!(&res, Nat)?)
    }

    /// Transfers amount of tokens from the account (caller, from_subaccount) to the account (to_principal, to_subaccount).
    pub async fn transfer(&self, args: TransferArgs) -> Result<u64> {
        println!("begin transfer");
        let res = self
            .agent
            .update(&self.ledger, "transfer")
            .with_arg(Encode!(&args)?)
            .call_and_wait()
            .await?;
        match Decode!(&res, Result<u64, TransferError>)? {
            Ok(nat) => Ok(nat),
            Err(err) => bail!("Transfer failed: {:?}", err),
        }
    }
}
