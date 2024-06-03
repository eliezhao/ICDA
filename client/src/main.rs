extern crate core;

use std::ops::Div;

use anyhow::Result;
use candid::Principal;
use cycles_minting_canister::SubnetSelection;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use ic_types::{PrincipalId, SubnetId};
use icp_ledger::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs};
use icrc_ledger_types::icrc1::account::Account;

use client::{CmcAgent, LedgerAgent};

const E8S: u64 = 100_000_000;

#[tokio::main]
async fn main() -> Result<()> {
    let identity = BasicIdentity::from_pem_file("identity.pem").unwrap();
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .with_identity(identity)
        .build()?;

    let ledger = LedgerAgent::new(agent.clone());
    let cmc = CmcAgent::new(agent.clone());

    // get account balance
    get_account_balance(agent.clone(), ledger.clone()).await;

    // first subnet: nl6hn-ja4yw-wvmpy-3z2jx-ymc34-pisx3-3cp5z-3oj4a-qzzny-jbsv3-4qe
    // second subnet:  opn46-zyspe-hhmyp-4zu6u-7sbrh-dok77-m7dch-im62f-vyimr-a3n2c-4ae
    let subnet_id = SubnetId::from(PrincipalId::from(
        Principal::from_text("opn46-zyspe-hhmyp-4zu6u-7sbrh-dok77-m7dch-im62f-vyimr-a3n2c-4ae")
            .unwrap(),
    ));

    println!("Create Canister in Subnet ID: {}", subnet_id.to_string());
    // create canister in specific subnet
    let canister_id = create_canister_in_specific_subnet(cmc, ledger, subnet_id).await?;
    println!("canister id: {}", canister_id.to_string());
    Ok(())
}

async fn get_account_balance(agent: Agent, ledger: LedgerAgent) -> Result<()> {
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

// failed block : 12109662;
// return canister id
async fn create_canister_in_specific_subnet(
    cmc_agent: CmcAgent,
    ledger_agent: LedgerAgent,
    subnet: SubnetId,
) -> Result<Principal> {
    // my identity principal id
    let pid = PrincipalId::from(cmc_agent.agent.get_principal().unwrap());
    // cmc sub-account from my principal
    let to_subaccount = Subaccount::from(&pid);
    let cmc_id = PrincipalId::from(cmc_agent.cmc);

    let block_index = transfer_to_cmc(ledger_agent, cmc_id, to_subaccount).await?;
    let subnet_selection = Some(SubnetSelection::Subnet { subnet });

    // notify cmc to create canister
    let arg = cycles_minting_canister::NotifyCreateCanister {
        block_index,
        controller: pid,
        subnet_type: None,
        subnet_selection,
        settings: None,
    };

    let cid = cmc_agent.notify_create_canister(arg).await?;
    println!("Canister ID: {:?}", cid.to_string());

    Ok(cid.get().0)
}

async fn transfer_to_cmc(
    ledger_agent: LedgerAgent,
    cmc_id: PrincipalId,
    to_subaccount: Subaccount,
) -> Result<u64> {
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
