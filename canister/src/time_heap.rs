use std::cmp::Reverse;
use std::collections::BinaryHeap;

use ic_cdk::print;

use crate::blob_id::BlobId;

const CANISTER_THRESHOLD: usize = 24; //30240

pub struct TimeHeap {
    heap: BinaryHeap<Reverse<BlobId>>,
}

impl TimeHeap {
    pub fn new() -> Self {
        TimeHeap {
            heap: BinaryHeap::new(),
        }
    }

    pub fn insert(&mut self, item: BlobId) {
        print(format!("Insert item to time heap: {:?}", item));
        self.heap.push(Reverse(item));
    }

    pub fn remove_expired(&mut self) -> Option<Reverse<BlobId>> {
        // 如果数量超过了阈值，就删除最早的blob
        if self.heap.len() > CANISTER_THRESHOLD {
            let expired_item = self.heap.pop();
            print(format!("Remove expired item: {:?}", expired_item));
            return expired_item;
        }
        None
    }
}

#[cfg(test)]
mod test {
    use std::thread::sleep;
    use std::time::{Duration, SystemTime, UNIX_EPOCH};

    use crate::blob_id::BlobId;

    use super::*;

    // before running this test, set CANISTER_THRESHOLD = 1;
    #[test]
    fn test_time_heap() {
        let mut heap = TimeHeap::new();
        // add the first blob
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let first_blob = BlobId {
            digest: [0; 32],
            timestamp: now,
        };
        heap.insert(first_blob.clone());
        assert_eq!(heap.heap.len(), 1);

        sleep(Duration::from_secs(1));

        // add the second blob
        let now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_nanos();
        let second_blob = BlobId {
            digest: [0; 32],
            timestamp: now,
        };
        heap.insert(second_blob.clone());
        assert_eq!(heap.heap.len(), 2);

        assert_eq!(heap.remove_expired().unwrap(), Reverse(first_blob));
        assert_eq!(heap.heap.len(), 1);
        assert_eq!(heap.remove_expired(), None);
    }
}
