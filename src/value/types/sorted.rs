// audited: 2026-09-08
//! Lookup over an immutable struct's entries: a binary search, because the
//! entries are sorted by key.
//!
//! docs/impl/values.md
//!
//! An immutable struct is a sorted `RegionSlice<(TableKey, Value)>`, so every
//! question about it is a binary search over `TableKey`'s comparator. The two
//! that produce a new struct answer with a `Vec` the caller allocates from.

use crate::value::Value;

use super::TableKey;

/// Look up a key in a sorted struct slice by binary search.
#[inline]
pub fn sorted_struct_get<'a>(
    entries: &'a [(TableKey, Value)],
    key: &TableKey,
) -> Option<&'a Value> {
    entries
        .binary_search_by(|(k, _)| k.cmp(key))
        .ok()
        .map(|i| &entries[i].1)
}

/// Check if a sorted struct slice contains a key.
#[inline]
pub fn sorted_struct_contains(entries: &[(TableKey, Value)], key: &TableKey) -> bool {
    entries.binary_search_by(|(k, _)| k.cmp(key)).is_ok()
}

/// Insert or update a key in a sorted Vec, maintaining sort order.
/// Returns a new Vec (for immutable struct operations).
pub fn sorted_struct_insert(
    entries: &[(TableKey, Value)],
    key: TableKey,
    value: Value,
) -> Vec<(TableKey, Value)> {
    let mut result = entries.to_vec();
    match result.binary_search_by(|(k, _)| k.cmp(&key)) {
        Ok(i) => result[i].1 = value,
        Err(i) => result.insert(i, (key, value)),
    }
    result
}

/// Remove a key from a sorted slice, returning a new Vec.
pub fn sorted_struct_remove(
    entries: &[(TableKey, Value)],
    key: &TableKey,
) -> Vec<(TableKey, Value)> {
    let mut result = entries.to_vec();
    if let Ok(i) = result.binary_search_by(|(k, _)| k.cmp(key)) {
        result.remove(i);
    }
    result
}
