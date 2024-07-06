//! 集成测试
//! 1. 存储blob，存储12个
//! 2. 获取blb，获取12个
//! 3. 获取confirmation

use std::collections::HashSet;
use std::sync::Arc;

use candid::Principal;
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use tokio::join;

use client::canister_interface::{BlobKey, ICStorage, CANISTER_COLLECTIONS, SIGNATURE_CANISTER};
use client::signature::{SignatureCanister, SignatureCanisterConfig};
use client::storage::{StorageCanister, StorageCanisterConfig};
use client::{get_from_canister, put_to_canister, verify_confirmation};

#[tokio::main]
async fn main() {
    let key_path = "client/test/blob_key.json";
    let identity_path = "identity/identity.pem";

    // basic setup
    let identity = BasicIdentity::from_pem_file(identity_path).unwrap();
    let agent = Arc::new(
        Agent::builder()
            .with_identity(identity)
            .with_url("https://ic0.app")
            .build()
            .unwrap(),
    );
    let signature_cid = Principal::from_text(SIGNATURE_CANISTER).unwrap();
    let storage_canisters = CANISTER_COLLECTIONS
        .iter()
        .map(|c| {
            c.iter()
                .map(|x| Principal::from_text(x).unwrap())
                .collect::<Vec<Principal>>()
        })
        .flatten()
        .collect::<Vec<_>>();

    let owner = agent.get_principal().unwrap();
    let signature = SignatureCanister::new(signature_cid, agent.clone());

    // update signature config: batch confirmation = 1
    let signature_config = SignatureCanisterConfig {
        confirmation_batch_size: 6,
        confirmation_live_time: 1,
        da_canisters: HashSet::from_iter(storage_canisters),
        owner,
    };

    match signature.update_config(&signature_config).await {
        Ok(_) => println!("update signature config success"),
        Err(e) => eprintln!("update signature config failed: {}", e),
    }

    // update storage canister config:
    let storage_canister_config = StorageCanisterConfig {
        owner,
        signature_canister: Principal::from_text(SIGNATURE_CANISTER).unwrap(),
        query_response_size: 2621440,
        canister_storage_threshold: 6,
    };

    let storage_1 = StorageCanister::new(
        Principal::from_text("hxctj-oiaaa-aaaap-qhltq-cai").unwrap(),
        agent.clone(),
    );

    let storage_2 = StorageCanister::new(
        Principal::from_text("v3y75-6iaaa-aaaak-qikaa-cai").unwrap(),
        agent.clone(),
    );

    let _ = join!(
        storage_1.update_config(&storage_canister_config),
        storage_2.update_config(&storage_canister_config)
    );

    println!("updated storage canister config");
    let mut storage = ICStorage::new(identity_path).unwrap();

    // 测试存储blob，6个
    println!("{}", "*".repeat(30));
    println!("start test save blob");
    match put_to_canister(6, key_path.to_string(), &mut storage).await {
        Ok(_) => println!("put to canister success"),
        Err(e) => eprintln!("put to canister failed: {}", e),
    }

    // 测试获取blob，6个
    println!("{}", "*".repeat(30));
    println!("start test get blob");
    match get_from_canister(key_path.to_string(), &storage).await {
        Ok(_) => println!("get from canister success"),
        Err(e) => eprintln!("get from canister failed: {}", e),
    }

    // 测试获取confirmation
    println!("{}", "*".repeat(30));
    println!("start test verify confirmation");
    match verify_confirmation(key_path.to_string()).await {
        Ok(_) => println!("verify confirmation success"),
        Err(e) => eprintln!("verify confirmation failed: {}", e),
    }

    let blob_key =
        serde_json::from_str::<Vec<BlobKey>>(&std::fs::read_to_string(key_path).unwrap()).unwrap();

    // put 7th blob
    println!("{}", "*".repeat(30));
    println!("start test save 7th blob");
    match put_to_canister(1, key_path.to_string(), &mut storage).await {
        Ok(_) => println!("put to canister success"),
        Err(e) => eprintln!("put to canister failed: {}", e),
    }

    // get 1th blob
    // 第一个应该已经retired
    println!("{}", "*".repeat(30));
    println!("start test get 1th blob");
    match storage_1.get_blob(blob_key[0].digest).await {
        Ok(_) => println!("get from canister success"),
        Err(e) => eprintln!("get from canister failed: {}", e),
    };

    // 再放5个blob
    println!("{}", "*".repeat(30));
    println!("start test save 5 blob");
    match put_to_canister(5, key_path.to_string(), &mut storage).await {
        Ok(_) => println!("put to canister success"),
        Err(e) => eprintln!("put to canister failed: {}", e),
    }

    // 获取confirmation
    // 前面6个confirmation应该全部是invalid，后面6个应该是正常的
    println!("{}", "*".repeat(30));
    println!("start test verify confirmation");
    match verify_confirmation(key_path.to_string()).await {
        Ok(_) => println!("verify confirmation success"),
        Err(e) => eprintln!("verify confirmation failed: {}", e),
    }
}
