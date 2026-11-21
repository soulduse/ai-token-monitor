//! Value interning for parsed usage records.
//!
//! A provider keeps one record per API call in memory (cross-file dedup needs
//! them), and a heavy user has several hundred thousand per month. Fields such
//! as the model id, session id, working directory and tool list repeat across
//! almost all of those records, so storing each as its own heap `String` cost
//! over 1 KB per record. Interning makes every record share one allocation per
//! distinct value.

use std::borrow::Borrow;
use std::collections::HashSet;
use std::hash::Hash;
use std::sync::{Arc, Mutex};

use super::resilience::lock_unpoisoned;

/// A set of shared values, usable as a `static`.
pub(crate) struct Interner<T: ?Sized> {
    values: Mutex<Option<HashSet<Arc<T>>>>,
}

impl<T: ?Sized + Eq + Hash> Interner<T> {
    pub(crate) const fn new() -> Self {
        Self { values: Mutex::new(None) }
    }

    fn get_or_insert<Q>(&self, value: &Q, make: impl FnOnce() -> Arc<T>) -> Arc<T>
    where
        Arc<T>: Borrow<Q>,
        Q: ?Sized + Eq + Hash,
    {
        let mut guard = lock_unpoisoned(&self.values);
        let values = guard.get_or_insert_with(HashSet::new);
        if let Some(existing) = values.get(value) {
            return Arc::clone(existing);
        }
        let shared = make();
        values.insert(Arc::clone(&shared));
        shared
    }
}

impl Interner<str> {
    /// The shared copy of `value`, allocated on first sight.
    pub(crate) fn intern(&self, value: &str) -> Arc<str> {
        self.get_or_insert(value, || Arc::from(value))
    }
}

impl<T: Eq + Hash> Interner<T> {
    /// The shared copy of `value`; `value` itself is dropped when one exists.
    pub(crate) fn intern_value(&self, value: T) -> Arc<T> {
        if let Some(existing) = lock_unpoisoned(&self.values)
            .as_ref()
            .and_then(|values| values.get(&value))
        {
            return Arc::clone(existing);
        }
        let shared = Arc::new(value);
        self.get_or_insert(&*shared, || Arc::clone(&shared))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn interned_strings_share_one_allocation() {
        let interner: Interner<str> = Interner::new();
        let first = interner.intern("claude-opus-4-8");
        let second = interner.intern(&String::from("claude-opus-4-8"));
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &interner.intern("claude-sonnet-5")));
    }

    #[test]
    fn interned_values_share_one_allocation() {
        let interner: Interner<Vec<String>> = Interner::new();
        let first = interner.intern_value(vec!["Bash".to_string()]);
        let second = interner.intern_value(vec!["Bash".to_string()]);
        assert!(Arc::ptr_eq(&first, &second));
        assert!(!Arc::ptr_eq(&first, &interner.intern_value(Vec::new())));
    }
}
