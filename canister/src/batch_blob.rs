use ic_cdk::print;

use crate::blob_id::BlobId;

const BATCH_SIZE: usize = 12; // 12 个 blob的信息，后续看怎么出proof然后构建签名之类的

#[derive(Clone)]
pub struct BatchCommit {
    batch: [BlobId; BATCH_SIZE],
    current_index: usize,
}

impl BatchCommit {
    // Constructor
    pub fn new() -> Self {
        BatchCommit {
            batch: [BlobId::new(); BATCH_SIZE],
            current_index: 0,
        }
    }

    // if return Some(blob_ids), then should call to get signature
    pub fn insert(&mut self, blob_id: BlobId) -> Option<[BlobId; BATCH_SIZE]> {
        self.current_index %= BATCH_SIZE;
        self.batch[self.current_index] = blob_id;
        self.current_index += 1;

        if self.current_index == BATCH_SIZE {
            print(format!("commit batch: {:?}", self.batch));
            Some(self.batch)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_commit() {
        let mut commit = BatchCommit::new();

        let mut blobs = vec![BlobId::new(); BATCH_SIZE];
        let mut index = 0;
        for i in &mut blobs {
            i.timestamp = index;
            index += 1;
        }

        for i in 0..BATCH_SIZE {
            let blob_id = blobs[i];
            if i == BATCH_SIZE - 1 {
                let res = commit.insert(blob_id);
                assert!(res.is_some());
                assert_eq!(res.unwrap().len(), BATCH_SIZE);

                for i in 0..BATCH_SIZE {
                    assert_eq!(commit.batch[i], blobs[i]);
                }
            }
            assert!(commit.insert(blob_id).is_none());
        }
    }
}
