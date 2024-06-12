extern crate ic_stable_structures;

use std::cell::RefMut;

use candid::CandidType;
use ic_stable_structures::{StableMinHeap, Storable};
use serde::{Deserialize, Serialize};

use crate::blob_id::BlobId;
use crate::Memory;

pub const CANISTER_THRESHOLD: u64 = 30240;

// 插入新的key到time heap
pub fn handle_time_heap(
    mut heap: RefMut<StableMinHeap<BlobId, Memory>>,
    digest: String,
    timestamp: u128,
) -> Option<BlobId> {
    let blob_id = BlobId { digest, timestamp };

    // 插入heap
    insert_id(&mut heap, blob_id.clone());

    // 删除过期的blob, 返回过期的blob
    remove_expired(&mut heap)
}

pub fn insert_id(heap: &mut RefMut<StableMinHeap<BlobId, Memory>>, item: BlobId) {
    print!(
        "Insert {:?} to time heap: result: {:?}\n",
        item,
        heap.push(&item)
    )
}

pub fn remove_expired(heap: &mut RefMut<StableMinHeap<BlobId, Memory>>) -> Option<BlobId> {
    // 如果数量超过了阈值，就删除最早的blob
    if heap.len() > CANISTER_THRESHOLD {
        let expired_item = heap.pop();
        return expired_item;
    }
    None
}

#[cfg(test)]
mod test {
    use std::cell::RefCell;
    use std::thread::sleep;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use ic_stable_structures::memory_manager::{MemoryId, MemoryManager};
    use ic_stable_structures::DefaultMemoryImpl;

    use crate::blob_id::BlobId;

    use super::*;

    // before running this test, set CANISTER_THRESHOLD = 1;
    #[test]
    fn test_time_heap() {
        let memory_mng: RefCell<MemoryManager<DefaultMemoryImpl>> =
            RefCell::new(MemoryManager::init(DefaultMemoryImpl::default()));

        let heap: RefCell<StableMinHeap<BlobId, Memory>> =
            RefCell::new(StableMinHeap::init(memory_mng.borrow().get(MemoryId::new(0))).unwrap());

        // add the first blob
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let first_blob = BlobId {
            digest: "first blob".to_string(),
            timestamp: now,
        };

        super::insert_id(&mut heap.borrow_mut(), first_blob.clone());
        assert_eq!(heap.borrow().len(), 1);

        sleep(Duration::from_secs(1));

        // add the second blob
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let second_blob = BlobId {
            digest: "second blob".to_string(),
            timestamp: now,
        };
        super::insert_id(&mut heap.borrow_mut(), second_blob.clone());
        assert_eq!(heap.borrow().len(), 2);

        assert_eq!(
            super::remove_expired(&mut heap.borrow_mut()).unwrap(),
            first_blob
        );
        assert_eq!(heap.borrow().len(), 1);
        assert_eq!(super::remove_expired(&mut heap.borrow_mut()), None);
    }
}
