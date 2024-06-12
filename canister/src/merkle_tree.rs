use rs_merkle::algorithms::Sha256;
use rs_merkle::{Hasher, MerkleProof, MerkleTree};

fn example() {
    // 叶子节点值，这里使用示例字符串，实际使用时应为 [u8; 32] 类型的哈希值
    let leaf_values = ["a", "b", "c", "d", "e", "f"];
    let leaves: Vec<[u8; 32]> = leaf_values
        .iter()
        .map(|x| Sha256::hash(x.as_bytes()))
        .collect();

    // 创建 Merkle 树
    let merkle_tree = MerkleTree::<Sha256>::from_leaves(&leaves);

    // 需要证明的叶子节点索引
    let indices_to_prove = vec![3, 4];
    let leaves_to_prove = leaves.get(3..5).expect("can't get leaves to prove");

    // 生成 Merkle 证明
    let merkle_proof = merkle_tree.proof(&indices_to_prove);

    // 获取 Merkle 根
    let merkle_root = merkle_tree.root().expect("couldn't get the merkle root");

    // 序列化证明以传递给客户端
    let proof_bytes = merkle_proof.to_bytes();

    // 在客户端解析证明
    let proof = MerkleProof::<Sha256>::try_from(proof_bytes).expect("Invalid proof");

    // 验证 Merkle 证明
    assert!(proof.verify(
        merkle_root,
        &indices_to_prove,
        leaves_to_prove,
        leaves.len(),
    ));
}

pub fn insert_node() {}

pub fn get_proof() {}

pub fn get_root() {}
