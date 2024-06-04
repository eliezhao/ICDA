use std::ops::Div;
use std::str::FromStr;

use candid::{CandidType, Decode, Encode, Principal};
use cycles_minting_canister::SubnetSelection;
use ic_agent::Agent;
use ic_management_canister_types::{CanisterSettingsArgsBuilder, LogVisibility};
use ic_types::{PrincipalId, SubnetId};
use icp_ledger::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs};
use icrc_ledger_types::icrc1::account::Account;
use serde::{Deserialize, Serialize};

use crate::cmc::CmcAgent;
use crate::ledger::LedgerAgent;

pub mod cmc;
pub mod ledger;
pub mod upload;

pub const LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const CMC: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";

// todo : 选10个合适的subnet，至少120G可用stable memory
pub const SUBNETS: [&str; 10] =
    ["opn46-zyspe-hhmyp-4zu6u-7sbrh-dok77-m7dch-im62f-vyimr-a3n2c-4ae"; 10];

pub async fn get_account_balance(agent: Agent, ledger: LedgerAgent) -> anyhow::Result<()> {
    let account = Account::from(agent.get_principal().unwrap());
    let balance = ledger.balance_of(account).await?;
    println!("原始balance: {}", balance.to_string());
    let balance = balance.div(100_000_000usize);
    let account_id = AccountIdentifier::from(agent.get_principal().unwrap());
    println!(
        "\
        Principal: {},\n
        Account ID: {:?},\n 
        Balance: {:?}",
        agent.get_principal().unwrap().to_string(),
        account_id.to_string(),
        balance.to_string()
    );
    Ok(())
}

pub async fn create_canister_in_specific_subnet(
    cmc_agent: CmcAgent,
    ledger_agent: LedgerAgent,
    subnet: SubnetId,
) -> anyhow::Result<Principal> {
    // my identity principal id
    let pid = PrincipalId::from(cmc_agent.agent.get_principal().unwrap());
    // cmc sub-account from my principal
    let to_subaccount = Subaccount::from(&pid);
    let cmc_id = PrincipalId::from(cmc_agent.cmc);

    let block_index = transfer_to_cmc(ledger_agent, cmc_id, to_subaccount).await?;
    let subnet_selection = Some(SubnetSelection::Subnet { subnet });

    let settings = Some(
        CanisterSettingsArgsBuilder::new()
            .with_controller(pid)
            .with_log_visibility(LogVisibility::Controllers)
            .with_freezing_threshold(14 * 24 * 60 * 60) // 14 days
            .build(),
    );

    // notify cmc to create canister
    let arg = cycles_minting_canister::NotifyCreateCanister {
        block_index,
        controller: pid,
        subnet_type: None,
        subnet_selection,
        settings,
    };

    let cid = cmc_agent.notify_create_canister(arg).await?;
    println!("Canister ID: {:?}", cid.to_string());

    Ok(cid.get().0)
}

pub async fn transfer_to_cmc(
    ledger_agent: LedgerAgent,
    cmc_id: PrincipalId,
    to_subaccount: Subaccount,
) -> anyhow::Result<u64> {
    let memo = Memo(1095062083);
    let fee = Tokens::from_e8s(10000);
    let amount = Tokens::from_e8s(10_000_000);
    let to = AccountIdentifier::new(cmc_id, Some(to_subaccount)).to_address();

    // transfer to destination account
    let transfer_args = TransferArgs {
        from_subaccount: None,
        to,
        amount, // 0.10 icp
        fee,
        created_at_time: None,
        memo, // create canister memo
    };

    let block_index = ledger_agent.transfer(transfer_args).await?;
    println!("transfer block index: {:?}", block_index);
    Ok(block_index)
}

#[derive(Deserialize, CandidType, Serialize, Debug)]
pub struct PublicKeyReply {
    pub public_key_hex: String,
}

// return hex encoded public key
pub async fn get_public_key(agent: &Agent, cid: &Principal) -> String {
    println!("get public key from : {}", cid.to_string());
    let res = agent
        .update(cid, "public_key")
        .with_arg(Encode!().unwrap())
        .call_and_wait()
        .await
        .expect("Failed to get public key");

    Decode!(&res, Result<PublicKeyReply, String>)
        .expect("Failed to decode public key")
        .unwrap()
        .public_key_hex
}

pub async fn get_signature(agent: &Agent, cid: &Principal) -> String {
    println!("get signature from : {}", cid.to_string());
    let res = agent
        .update(cid, "get_signature")
        .with_arg(Encode!().unwrap())
        .call_and_wait()
        .await
        .expect(format!("Failed to get signature from: {}", cid.to_string()).as_str());

    Decode!(&res, Result<PublicKeyReply, String>)
        .expect("Failed to decode public key")
        .unwrap()
        .public_key_hex
}

pub fn sha256(input: &String) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}
