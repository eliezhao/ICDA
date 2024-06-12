/// 上传处理三个事情
/// btree map
/// time heap
/// signature
use std::cell::RefMut;

use candid::{CandidType, Deserialize};
use ic_stable_structures::BTreeMap;
use serde::Serialize;

use crate::Memory;

// upload 用
#[derive(Deserialize, Serialize, CandidType, Debug, Clone)]
pub struct BlobChunk {
    /// Sha256 digest of the blob in hex format.
    pub digest: String, // hex encoded digest

    /// Time since epoch in nanos.
    pub timestamp: u128,

    /// blob总大小
    pub total: usize,

    /// The actual chunk.
    pub chunk: Vec<u8>,
}

// todo: 先方time heap，再放btree，这样可以直接把chunk放进来
// todo: 分片串行上传 handle
pub fn handle_upload(mut map: RefMut<BTreeMap<String, Vec<u8>, Memory>>, chunk: BlobChunk) {
    // 获取map中有无key - value
    if map.get(&chunk.digest).is_none() {
        // 没有，就insert，同时将vec的大小控制为总大小
        let value: Vec<u8> = Vec::with_capacity(chunk.total);
        map.insert(chunk.digest.clone(), value);
    }

    let mut value = map.get(&chunk.digest).unwrap();
    value.extend(chunk.chunk.clone());
    map.insert(chunk.digest.clone(), value);
}

// todo: get要获取chunk的blob size是否大于阈值，如果大于，就要分片获取

#[cfg(test)]
mod test {
    use std::cell::RefCell;

    use ic_stable_structures::memory_manager::{MemoryId, MemoryManager};
    use ic_stable_structures::{DefaultMemoryImpl, StableBTreeMap};

    use super::*;

    // test handle_upload function
    #[test]
    fn test_handle_upload() {
        let memory_mng = MemoryManager::init(DefaultMemoryImpl::default());
        let mut map: RefCell<StableBTreeMap<String, Vec<u8>, Memory>> =
            RefCell::new(StableBTreeMap::init(memory_mng.get(MemoryId::new(0))));
        let chunk_0 = BlobChunk {
            digest: "test".to_string(),
            timestamp: 0,
            total: 6,
            chunk: vec![1, 2, 3],
        };

        let chunk_1 = BlobChunk {
            digest: "test".to_string(),
            timestamp: 0,
            total: 6,
            chunk: vec![4, 5, 6],
        };

        super::handle_upload(map.borrow_mut(), chunk_0);
        super::handle_upload(map.borrow_mut(), chunk_1);
        assert_eq!(
            map.borrow().get(&"test".to_string()).unwrap(),
            vec![1, 2, 3, 4, 5, 6]
        );
    }
}
