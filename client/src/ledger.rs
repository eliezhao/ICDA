use std::str::FromStr;

use anyhow::bail;
use candid::{Decode, Encode, Nat, Principal};
use ic_agent::Agent;
use icp_ledger::TransferArgs;
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::TransferError;

use crate::LEDGER;

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
    pub async fn balance_of(&self, account: Account) -> anyhow::Result<Nat> {
        let res = self
            .agent
            .query(&self.ledger, "icrc1_balance_of")
            .with_arg(Encode!(&account)?)
            .call()
            .await?;

        Ok(Decode!(&res, Nat)?)
    }

    /// Transfers amount of tokens from the account (caller, from_subaccount) to the account (to_principal, to_subaccount).
    pub async fn transfer(&self, args: TransferArgs) -> anyhow::Result<u64> {
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
