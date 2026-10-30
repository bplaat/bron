/*
 * Copyright (c) 2026 Bastiaan van der Plaat
 *
 * SPDX-License-Identifier: MIT
 */

//! A map of `String` to `plist::Value` that preserves insertion order.

use std::fmt::{self, Debug};
use std::{ops, slice, vec};

use crate::Value;

/// A map of `String` to `plist::Value` that preserves insertion order.
#[derive(Clone, Default)]
pub struct Dictionary {
    entries: Vec<(String, Value)>,
}

impl Dictionary {
    /// Makes a new empty `Dictionary`.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn new() -> Self {
        Self {
            entries: Vec::new(),
        }
    }

    /// Returns a reference to the value corresponding to the key.
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.entries.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    /// Inserts a key-value pair into the dictionary, keeping the position of an existing key.
    ///
    /// Returns the previous value of the key, if any.
    pub fn insert(&mut self, k: String, v: Value) -> Option<Value> {
        match self.entries.iter_mut().find(|(key, _)| *key == k) {
            Some((_, value)) => Some(std::mem::replace(value, v)),
            None => {
                self.entries.push((k, v));
                None
            }
        }
    }

    /// Returns the number of elements in the dictionary.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Returns true if the dictionary contains no elements.
    #[allow(clippy::missing_const_for_fn)] // Upstream API is not const
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    /// Gets an iterator over the entries of the dictionary.
    pub fn iter(&self) -> Iter<'_> {
        Iter {
            iter: self.entries.iter(),
        }
    }
}

impl PartialEq for Dictionary {
    /// Dictionaries are equal when they contain the same entries, in any order.
    fn eq(&self, other: &Self) -> bool {
        self.len() == other.len() && self.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl ops::Index<&str> for Dictionary {
    type Output = Value;

    fn index(&self, index: &str) -> &Value {
        self.get(index).expect("no entry found for key")
    }
}

impl Debug for Dictionary {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_map().entries(self.iter()).finish()
    }
}

impl<K: Into<String>, V: Into<Value>> FromIterator<(K, V)> for Dictionary {
    fn from_iter<I: IntoIterator<Item = (K, V)>>(iter: I) -> Self {
        let mut dict = Self::new();
        dict.extend(iter.into_iter().map(|(k, v)| (k.into(), v.into())));
        dict
    }
}

impl Extend<(String, Value)> for Dictionary {
    fn extend<T: IntoIterator<Item = (String, Value)>>(&mut self, iter: T) {
        for (k, v) in iter {
            self.insert(k, v);
        }
    }
}

// MARK: Iterators
impl<'a> IntoIterator for &'a Dictionary {
    type Item = (&'a String, &'a Value);
    type IntoIter = Iter<'a>;

    fn into_iter(self) -> Self::IntoIter {
        self.iter()
    }
}

/// An iterator over a `plist::Dictionary`'s entries.
pub struct Iter<'a> {
    iter: slice::Iter<'a, (String, Value)>,
}

impl<'a> Iterator for Iter<'a> {
    type Item = (&'a String, &'a Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next().map(|(k, v)| (k, v))
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl ExactSizeIterator for Iter<'_> {}

impl IntoIterator for Dictionary {
    type Item = (String, Value);
    type IntoIter = IntoIter;

    fn into_iter(self) -> Self::IntoIter {
        IntoIter {
            iter: self.entries.into_iter(),
        }
    }
}

/// An owning iterator over a `plist::Dictionary`'s entries.
pub struct IntoIter {
    iter: vec::IntoIter<(String, Value)>,
}

impl Iterator for IntoIter {
    type Item = (String, Value);

    fn next(&mut self) -> Option<Self::Item> {
        self.iter.next()
    }

    fn size_hint(&self) -> (usize, Option<usize>) {
        self.iter.size_hint()
    }
}

impl ExactSizeIterator for IntoIter {}
