/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

// Small least recently used caches of native objects that are costly to create, like
// gradients and paths that are drawn every frame.

use std::sync::Arc;

use crate::canvas::Gradient;

pub(crate) struct LruCache<K, V> {
    // Least recently used first.
    entries: Vec<(K, V)>,
    capacity: usize,
}

impl<K: PartialEq, V> LruCache<K, V> {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self {
            entries: Vec::new(),
            capacity,
        }
    }

    /// The value of a key, created when it is missing. None when creating it fails.
    pub(crate) fn get_or_create(
        &mut self,
        key: K,
        create: impl FnOnce() -> Option<V>,
    ) -> Option<&V> {
        if let Some(index) = self.entries.iter().position(|(cached, _)| *cached == key) {
            self.entries[index..].rotate_left(1);
        } else {
            let value = create()?;
            if self.entries.len() == self.capacity {
                self.entries.remove(0);
            }
            self.entries.push((key, value));
        }
        self.entries.last().map(|(_, value)| value)
    }
}

/// A gradient that keys the native gradients of equal gradients.
#[derive(Clone)]
pub(crate) struct GradientKey(pub(crate) Arc<Gradient>);

impl PartialEq for GradientKey {
    fn eq(&self, other: &Self) -> bool {
        Arc::ptr_eq(&self.0, &other.0) || self.0 == other.0
    }
}

#[cfg(test)]
mod tests {
    use super::LruCache;

    #[test]
    fn evicts_the_least_recently_used_entry() {
        let mut cache = LruCache::new(2);
        cache.get_or_create(1, || Some('a'));
        cache.get_or_create(2, || Some('b'));
        assert_eq!(cache.get_or_create(1, || None), Some(&'a'));
        cache.get_or_create(3, || Some('c'));
        assert_eq!(cache.get_or_create(2, || None), None);
        assert_eq!(cache.get_or_create(1, || None), Some(&'a'));
        assert_eq!(cache.get_or_create(4, || None), None);
    }
}
