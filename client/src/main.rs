extern crate core;

use std::ops::Div;

use anyhow::Result;
use candid::Principal;
use cycles_minting_canister::SubnetSelection;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use ic_management_canister_types::{CanisterSettingsArgsBuilder, LogVisibility};
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
    let _ = get_account_balance(agent.clone(), ledger.clone()).await;

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

#[cfg(test)]
mod test {
    use hex;
    use secp256k1::ecdsa::Signature;
    use secp256k1::{Message, PublicKey, Secp256k1};

    pub fn sha256(input: &String) -> [u8; 32] {
        use sha2::Digest;
        let mut hasher = sha2::Sha256::new();
        hasher.update(input.as_bytes());
        hasher.finalize().into()
    }

    // sign "msg" as a message and call sign function to get signature
    // verify the signature with public key
    #[test]
    fn test_verify_signature() {
        let signature_hex = "9f93e1cd3c9ab2c1f4b9f42fafd23445b9c1c6928e4c4a9f69e3333befcdd5ea37eb1b2dd7b16172be05dab81c3ef1f91b25c3f8c2261217a8511e820aeced62";
        let message_bytes = sha256(&"msg".to_string()).to_vec();
        let public_key_hex = "0328bded9949481ac3d6deb772f7f6911625c425cdcd4e987bd70c969d46ef06b0";

        // Create a Secp256k1 context
        let secp = Secp256k1::verification_only();

        // Convert hex strings to bytes
        let signature_bytes = hex::decode(signature_hex).expect("Invalid hex in signature");
        let public_key_bytes = hex::decode(public_key_hex).expect("Invalid hex in public key");

        // Create Signature, Message and PublicKey objects
        let signature = Signature::from_compact(&signature_bytes).expect("Invalid signature");
        let message = Message::from_digest_slice(&message_bytes).expect("Invalid message");
        let public_key = PublicKey::from_slice(&public_key_bytes).expect("Invalid public key");

        // Verify the signature
        let res = secp.verify_ecdsa(&message, &signature, &public_key);
        assert!(res.is_ok());
    }
}
