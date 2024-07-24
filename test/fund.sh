#!/bin/bash

# 定义Canister ID数组
CANISTER_COLLECTIONS=(
    "hxctj-oiaaa-aaaap-qhltq-cai"
    "v3y75-6iaaa-aaaak-qikaa-cai"
    "nnw5b-eqaaa-aaaak-qiqaq-cai"
    "wcrzb-2qaaa-aaaap-qhpgq-cai"
    "y446g-jiaaa-aaaap-ahpja-cai"
    "hmqa7-byaaa-aaaam-ac4aq-cai"
    "jeizw-6yaaa-aaaal-ajora-cai"
    "vrk5x-dyaaa-aaaan-qmrsq-cai"
    "zhu6y-liaaa-aaaal-qjlmq-cai"
    "oyfj2-gaaaa-aaaak-akxdq-cai"
    "r2xtu-uiaaa-aaaag-alf6q-cai"
)

# 遍历数组并执行命令
for cid in "${CANISTER_COLLECTIONS[@]}"; do
    echo "Sending to $cid"
    dfx wallet --network ic send "$cid" 10_000_000_000_000
done

