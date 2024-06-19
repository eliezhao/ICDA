use candid::{CandidType, Decode, Encode};
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;

#[derive(CandidType, Deserialize, Debug)]
struct BurnArgs {
    canister_id: Principal,
    amount: u64,
}

#[derive(CandidType, Deserialize, Debug)]
enum BurnError {
    InsufficientBalance,
    InvalidTokenContract,
    NotSufficientLiquidity,
}

#[derive(CandidType, Deserialize, Debug)]
enum BurnResult {
    Ok(u64),
    Err(BurnError),
}

#[tokio::test]
async fn xtc2cycle() {
    let identity = BasicIdentity::from_pem_file("../identity.pem").unwrap();
    let agent = Agent::builder()
        .with_url("https://ic0.app")
        .with_identity(identity)
        .build()
        .unwrap();
    let canister_id = Principal::from_text("aanaa-xaaaa-aaaah-aaeiq-cai").unwrap();
    let to = Principal::from_text("v3y75-6iaaa-aaaak-qikaa-cai").unwrap();
    let res = agent
        .update(&canister_id, "burn")
        .with_arg(
            Encode!(&BurnArgs {
                canister_id: to,
                amount: 2 * E8S,
            })
            .unwrap(),
        )
        .call_and_wait()
        .await
        .expect("call to canister failed");
    println!("{:#?}", Decode!(&res, BurnResult).unwrap());
}
