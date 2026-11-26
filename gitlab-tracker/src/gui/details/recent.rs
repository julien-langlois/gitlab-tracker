//! A cache that keeps the entries used last, up to a number.

use std::collections::{HashMap, VecDeque};
use std::hash::Hash;

/// At most `max` entries: inserting one more drops the one used longest ago.
/// What is dropped is read again from GitLab if it is asked for again.
pub(super) struct Recent<K, V> {
    max: usize,
    map: HashMap<K, V>,
    /// The keys, the one used longest ago first.
    order: VecDeque<K>,
}

impl<K: Hash + Eq + Clone, V> Recent<K, V> {
    pub(super) fn new(max: usize) -> Self {
        Self {
            max: max.max(1),
            map: HashMap::new(),
            order: VecDeque::new(),
        }
    }

    pub(super) fn contains_key(&self, key: &K) -> bool {
        self.map.contains_key(key)
    }

    /// The entry, now the one used last.
    pub(super) fn get_mut(&mut self, key: &K) -> Option<&mut V> {
        if self.map.contains_key(key) {
            self.touch(key);
        }
        self.map.get_mut(key)
    }

    /// The entry, without counting as a use.
    pub(super) fn get(&self, key: &K) -> Option<&V> {
        self.map.get(key)
    }

    pub(super) fn insert(&mut self, key: K, value: V) {
        if self.map.insert(key.clone(), value).is_some() {
            self.touch(&key);
            return;
        }
        self.order.push_back(key);
        while self.map.len() > self.max {
            let Some(oldest) = self.order.pop_front() else {
                break;
            };
            self.map.remove(&oldest);
        }
    }

    pub(super) fn retain(&mut self, mut keep: impl FnMut(&K) -> bool) {
        self.map.retain(|k, _| keep(k));
        let map = &self.map;
        self.order.retain(|k| map.contains_key(k));
    }

    #[cfg(test)]
    pub(super) fn len(&self) -> usize {
        self.map.len()
    }

    fn touch(&mut self, key: &K) {
        if let Some(at) = self.order.iter().position(|k| k == key) {
            if let Some(k) = self.order.remove(at) {
                self.order.push_back(k);
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_entry_used_longest_ago_goes_first() {
        let mut cache = Recent::new(2);
        cache.insert("a", 1);
        cache.insert("b", 2);
        // "a" used again: "b" is now the oldest.
        assert_eq!(cache.get_mut(&"a"), Some(&mut 1));
        cache.insert("c", 3);
        assert!(cache.contains_key(&"a") && cache.contains_key(&"c"));
        assert!(!cache.contains_key(&"b"));
        assert_eq!(cache.len(), 2);
        // Replacing an entry keeps the count.
        cache.insert("c", 4);
        assert_eq!(cache.get(&"c"), Some(&4));
        assert_eq!(cache.len(), 2);
        cache.retain(|k| *k != "a");
        cache.insert("d", 5);
        cache.insert("e", 6);
        assert!(!cache.contains_key(&"c"));
        assert_eq!(cache.len(), 2);
    }
}
