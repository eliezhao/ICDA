extern crate core;

use std::io;

use anyhow::Result;
use candid::{CandidType, Decode, Deserialize, Encode, Principal};
use ic_agent::identity::BasicIdentity;
use ic_agent::Agent;
use rand::Rng;
use sha2::Digest;

use client::upload::ICStorage;

const E8S: u64 = 100_000_000;

#[tokio::main]
async fn main() -> Result<()> {
    let mut path = String::new();

    println!("entre identity.pem path:");
    io::stdin()
        .read_line(&mut path)
        .expect("Failed to read line");

    // 去除输入字符串末尾的换行符
    let path = path.trim();

    println!("开始测试");
    let mut da = ICStorage::new(path.to_string()).unwrap();

    // let mut rng = rand::thread_rng();
    // //准备4个blob
    // let mut batch_1 = vec![vec![0u8; 3 * 1024 * 1024]; 10]; // 10个3M
    // for i in &mut batch_1 {
    //     rng.fill(&mut i[..]);
    // }
    //
    // println!("{}", "-".repeat(20));
    // let mut response = Vec::new();
    //
    // for (index, item) in batch_1.iter().enumerate() {
    //     println!("第 {} 个Batch", index);
    //     let res = da.save_blob(item.to_vec()).await?;
    //     let raw = String::from_utf8(res).unwrap();
    //     let key = serde_json::from_str::<BlobKey>(&raw).unwrap();
    //     response.push(key)
    // }
    //
    // println!("{}begin sleep {}", "-".repeat(10), "-".repeat(10));
    // tokio::time::sleep(Duration::from_secs(600)).await;

    // 获取Blob
    // println!("{}", "-".repeat(20));
    // println!("获取Blob");
    // let mut batch_2 = Vec::new();
    // for (index, blob_key) in response.iter().enumerate() {
    //     println!("第 {} 个Batch", index);
    //     let res = da.get_blob(blob_key.clone()).await?;
    //     batch_2.push(res);
    // }

    // -------------

    // println!("{}", "-".repeat(20));
    // println!("验证Blob");
    // for (i, (a, b)) in batch_1.iter().zip(batch_2.iter()).enumerate() {
    //     let a_sha = sha2::Sha256::digest(a.as_slice());
    //     let b_sha = sha2::Sha256::digest(b.as_slice());
    //
    //     assert_eq!(a_sha, b_sha, "sha256: blob {} not equal", i);
    // }

    // 两个测试canister
    // let canister_1_principal = Principal::from_text("hxctj-oiaaa-aaaap-qhltq-cai").unwrap();
    // let canister_2_principal = Principal::from_text("v3y75-6iaaa-aaaak-qikaa-cai").unwrap();
    //
    // println!("{}", "-".repeat(20));
    // println!("获取两个Canister的Public Key");
    // // get public key
    // let canister_1_public_key = PublicKey::from_slice(
    //     &hex::decode(get_public_key(&agent, &canister_1_principal).await).unwrap(),
    // )
    // .unwrap();
    // let canister_2_public_key = PublicKey::from_slice(
    //     &hex::decode(get_public_key(&agent, &canister_2_principal).await).unwrap(),
    // )
    // .unwrap();
    //
    // println!("{}", "-".repeat(20));
    // println!("获取两个Canister的Signature");
    // // get hex encoded signatures
    // let canister_1_signature = Signature::from_compact(
    //     &hex::decode(get_signature(&agent, &canister_1_principal).await).unwrap(),
    // )
    // .unwrap();
    // let canister_2_signature = Signature::from_compact(
    //     &hex::decode(get_signature(&agent, &canister_2_principal).await).unwrap(),
    // )
    // .unwrap();
    //
    // // verify signatures
    // let msg = Message::from_digest_slice(
    //     &sha256(&"this is a message should be signed".to_string()).to_vec(),
    // )
    // .unwrap();
    // let secp = Secp256k1::verification_only();
    // if let Ok(_) = secp.verify_ecdsa(&msg, &canister_1_signature, &canister_1_public_key) {
    //     println!("Canister 1 signature is valid");
    // } else {
    //     println!("Canister 1 signature is invalid");
    // }
    //
    // if let Ok(_) = secp.verify_ecdsa(&msg, &canister_2_signature, &canister_2_public_key) {
    //     println!("Canister 2 signature is valid");
    // } else {
    //     println!("Canister 2 signature is invalid");
    // }
    //
    // // 放第25个进，第1个应该过期
    // let mut blob_25 = vec![0u8; 10];
    // rng.fill(&mut blob_25[..]);
    // let res = da.save_blob(blob_25.clone()).await?;
    // let raw = String::from_utf8(res.clone()).unwrap();
    // let _ = serde_json::from_str::<BlobId>(&raw).unwrap(); // recover key from raw string
    //
    // // get 25th blob
    // let res = da.get_blob(res).await?;
    // assert_eq!(blob_25, res);
    //
    // // get blob id = 0 from canister and should return empty vector
    // let res = da
    //     .get_blob(serde_json::to_string(&response[0])?.as_bytes().to_vec())
    //     .await?;
    // assert_eq!(res.len(), 0);

    Ok(())
}

#[cfg(test)]
mod test {
    use std::collections::HashMap;

    use hex;
    use num_bigint::BigUint;
    use rand::Rng;
    use secp256k1::ecdsa::Signature;
    use secp256k1::{Message, PublicKey, Secp256k1};
    use sha2::Digest;

    use client::sha256;

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

    // hash 确实会导致不均匀分布
    #[test]
    fn count_number() {
        let mut map = HashMap::new();
        for _ in 0..1000 {
            let mut rng = rand::thread_rng();
            let mut v = vec![0u8; 100];
            rng.fill(&mut v[..]);
            let mut hasher = sha2::Sha256::new();
            hasher.update(v);
            let hash: [u8; 32] = hasher.finalize().into();
            let num = (BigUint::from_bytes_be(&hash) % BigUint::from(20u32)).bits();

            let _ = map
                .get_mut(&num)
                .and_then(|count| Some(*count += 1))
                .or_else(|| {
                    map.insert(num, 1);
                    None
                });
        }

        // print all elements
        for (k, v) in map.iter() {
            println!("{:?} => {}", k, v);
        }
    }
}
