use std::ops::Div;
use std::str::FromStr;

use anyhow::bail;
use candid::{Decode, Encode, Nat, Principal};
use cycles_minting_canister::{NotifyCreateCanister, NotifyError, SubnetSelection};
use ic_agent::Agent;
use ic_management_canister_types::{CanisterSettingsArgsBuilder, LogVisibility};
use ic_types::{CanisterId, PrincipalId, SubnetId};
use icp_ledger::{AccountIdentifier, Memo, Subaccount, Tokens, TransferArgs};
use icrc_ledger_types::icrc1::account::Account;
use icrc_ledger_types::icrc1::transfer::TransferError;

pub const LEDGER: &str = "ryjl3-tyaaa-aaaaa-aaaba-cai";
pub const CMC: &str = "rkp4c-7iaaa-aaaaa-aaaca-cai";

pub const SUBNETS: [&str; 9] = [
    "nl6hn-ja4yw-wvmpy-3z2jx-ymc34-pisx3-3cp5z-3oj4a-qzzny-jbsv3-4qe",
    "opn46-zyspe-hhmyp-4zu6u-7sbrh-dok77-m7dch-im62f-vyimr-a3n2c-4ae",
    "3hhby-wmtmw-umt4t-7ieyg-bbiig-xiylg-sblrt-voxgt-bqckd-a75bf-rqe",
    "4ecnw-byqwz-dtgss-ua2mh-pfvs7-c3lct-gtf4e-hnu75-j7eek-iifqm-sqe",
    "6pbhf-qzpdk-kuqbr-pklfa-5ehhf-jfjps-zsj6q-57nrl-kzhpd-mu7hc-vae",
    "cv73p-6v7zi-u67oy-7jc3h-qspsz-g5lrj-4fn7k-xrax3-thek2-sl46v-jae",
    "e66qm-3cydn-nkf4i-ml4rb-4ro6o-srm5s-x5hwq-hnprz-3meqp-s7vks-5qe",
    "k44fs-gm4pv-afozh-rs7zw-cg32n-u7xov-xqyx3-2pw5q-eucnu-cosd4-uqe",
    "lspz2-jx4pu-k3e7p-znm7j-q4yum-ork6e-6w4q6-pijwq-znehu-4jabe-kqe",
];

// "io67a-2jmkw-zup3h-snbwi-g6a5n-rm5dn-b6png-lvdpl-nqnto-yih6l-gqe", // 1st
// "nl6hn-ja4yw-wvmpy-3z2jx-ymc34-pisx3-3cp5z-3oj4a-qzzny-jbsv3-4qe", // 2st

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

    /// Transfers number of tokens from the account (caller, from_subaccount) to the account (to_principal, to_subaccount).
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

pub async fn get_account_balance(agent: Agent, ledger: LedgerAgent) -> anyhow::Result<()> {
    let account = Account::from(agent.get_principal().unwrap());
    let balance = ledger.balance_of(account).await?;
    println!("原始balance: {}", balance);
    let balance = balance.div(100_000_000usize);
    let account_id = AccountIdentifier::from(agent.get_principal().unwrap());
    println!(
        "\
        Principal: {},\n
        Account ID: {:?},\n
        Balance: {:?}",
        agent.get_principal().unwrap(),
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
            .with_controllers(vec![pid])
            .with_log_visibility(LogVisibility::Controllers)
            .with_freezing_threshold(14 * 24 * 60 * 60) // 14 days
            .build(),
    );

    // notify cmc to create canister
    let arg = NotifyCreateCanister {
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

pub fn sha256(input: &String) -> [u8; 32] {
    use sha2::Digest;
    let mut hasher = sha2::Sha256::new();
    hasher.update(input.as_bytes());
    hasher.finalize().into()
}

#[tokio::test]
async fn create() {
    use ic_agent::identity::BasicIdentity;
    let identity = BasicIdentity::from_pem_file("../identity/identity.pem").unwrap();
    let agent = Agent::builder()
        .with_identity(identity)
        .with_url("https://ic0.app")
        .build()
        .unwrap();
    let ledger = LedgerAgent::new(agent.clone());
    let cmc = CmcAgent::new(agent.clone());

    for subnet in SUBNETS {
        let subnet_id = SubnetId::new(PrincipalId::from_str(subnet).unwrap());
        match create_canister_in_specific_subnet(cmc.clone(), ledger.clone(), subnet_id).await {
            Ok(cid) => {
                println!("Canister ID: {:?}", cid.to_text());
            }
            Err(e) => {
                eprintln!("Error: {:?}", e);
            }
        }
    }
}
