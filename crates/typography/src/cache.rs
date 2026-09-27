//! Byte-bounded font-local memoization. Clearing a font database also clears
//! these entries, since fontdb IDs are meaningful only inside that database.
use std::{
    collections::{HashMap, VecDeque},
    sync::Mutex,
};

pub(crate) struct Cache<T> {
    inner: Mutex<Entries<T>>,
}
struct Entries<T> {
    map: HashMap<Vec<u8>, T>,
    order: VecDeque<(Vec<u8>, usize)>,
    bytes: usize,
}
impl<T> Default for Cache<T> {
    fn default() -> Self {
        Self {
            inner: Mutex::new(Entries {
                map: HashMap::new(),
                order: VecDeque::new(),
                bytes: 0,
            }),
        }
    }
}
impl<T: Clone> Cache<T> {
    #[cfg(test)]
    pub fn entry_count(&self) -> usize {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map
            .len()
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }
    pub fn get(&self, key: &[u8]) -> Option<T> {
        self.inner
            .lock()
            .unwrap_or_else(|e| e.into_inner())
            .map
            .get(key)
            .cloned()
    }
    pub fn insert(&self, key: Vec<u8>, value: T, size: usize) {
        const BUDGET: usize = 8 << 20;
        let size = size + key.len() * 2 + 128;
        if size > BUDGET {
            return;
        }
        let mut cache = self.inner.lock().unwrap_or_else(|e| e.into_inner());
        if cache.map.contains_key(&key) {
            return;
        }
        while cache.bytes + size > BUDGET {
            let Some((key, size)) = cache.order.pop_front() else {
                break;
            };
            cache.map.remove(&key);
            cache.bytes -= size;
        }
        cache.bytes += size;
        cache.order.push_back((key.clone(), size));
        cache.map.insert(key, value);
    }
}
