//! Small byte-bounded geometry memo. Values are immutable and shared by tile jobs.
use std::{
    collections::{HashMap, VecDeque},
    sync::Arc,
};

pub(crate) struct Memo<T> {
    entries: HashMap<[u8; 32], Arc<T>>,
    order: VecDeque<([u8; 32], usize)>,
    bytes: usize,
    budget: usize,
}
impl<T> Memo<T> {
    pub fn new(budget: usize) -> Self {
        Self {
            entries: HashMap::new(),
            order: VecDeque::new(),
            bytes: 0,
            budget,
        }
    }
    pub fn get(&self, key: &[u8; 32]) -> Option<Arc<T>> {
        self.entries.get(key).cloned()
    }
    pub fn insert(&mut self, key: [u8; 32], value: Arc<T>, bytes: usize) {
        let bytes = bytes + 160; // map/queue/key/Arc bookkeeping
        if bytes > self.budget {
            return;
        }
        if let Some(index) = self.order.iter().position(|(k, _)| *k == key) {
            let (_, size) = self.order.remove(index).unwrap();
            self.bytes -= size;
            self.entries.remove(&key);
        }
        while self.bytes + bytes > self.budget {
            let Some((key, size)) = self.order.pop_front() else {
                break;
            };
            self.entries.remove(&key);
            self.bytes -= size;
        }
        self.bytes += bytes;
        self.order.push_back((key, bytes));
        self.entries.insert(key, value);
    }
    pub fn clear(&mut self) {
        self.entries.clear();
        self.order.clear();
        self.bytes = 0;
    }
}
