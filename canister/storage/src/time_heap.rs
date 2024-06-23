extern crate ic_stable_structures;

use std::cell::RefMut;

use ic_stable_structures::StableMinHeap;

use crate::blob_id::BlobId;
use crate::Memory;

pub const CANISTER_THRESHOLD: u32 = 30240;

// 1. 将新的node插入heap
// 2. 如果到canister阈值，则每进来一个blob，就pop出过期的item
pub fn handle_time_heap(
    mut heap: RefMut<StableMinHeap<BlobId, Memory>>,
    digest: String,
    timestamp: u128,
) -> Option<BlobId> {
    let blob_id = BlobId { digest, timestamp };

    // 插入heap并输出log
    print!(
        "Insert {:?} to time heap: result: {:?}",
        blob_id,
        heap.push(&blob_id)
    );

    // 删除过期的blob, 返回过期的blob
    if heap.len() > CANISTER_THRESHOLD as u64 {
        let expired_item = heap.pop();
        return expired_item;
    }
    None
}
