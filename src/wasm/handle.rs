//! Handle table: maps opaque u64 handles to Rc<HeapObject>.
//!
//! WASM code sees heap values as `(tag: i64, payload: i64)` where
//! `payload` is a handle index. The host resolves handles to actual
//! `Value` objects for primitive dispatch and runtime operations.
//!
//! Handles are allocated from a free-list for O(1) insert/remove.

use crate::value::Value;

/// Maps u64 handle indices to Elle Values.
///
/// Handle 0 is reserved (never allocated). Freed handles go onto a
/// free-list for reuse. The table grows as needed.
pub struct HandleTable {
    /// Slot 0 is reserved. `entries[i]` holds the Value for handle `i`,
    /// or `None` if the slot is free.
    entries: Vec<Option<Value>>,
    /// Free-list of reusable slot indices.
    free: Vec<u64>,
    /// Next fresh handle (if free-list is empty).
    next: u64,
}

impl HandleTable {
    pub fn new() -> Self {
        HandleTable {
            entries: vec![None], // slot 0 reserved
            free: Vec::new(),
            next: 1,
        }
    }

    /// Insert a value, returning its handle.
    pub fn insert(&mut self, value: Value) -> u64 {
        if let Some(idx) = self.free.pop() {
            self.entries[idx as usize] = Some(value);
            idx
        } else {
            let idx = self.next;
            self.next += 1;
            if idx as usize >= self.entries.len() {
                self.entries.resize(idx as usize + 1, None);
            }
            self.entries[idx as usize] = Some(value);
            idx
        }
    }

    /// Look up a handle. Panics if invalid.
    pub fn get(&self, handle: u64) -> Value {
        self.entries[handle as usize].expect("HandleTable::get: invalid or freed handle")
    }

    /// Remove a handle, returning its value.
    pub fn remove(&mut self, handle: u64) -> Value {
        let value = self.entries[handle as usize]
            .take()
            .expect("HandleTable::remove: invalid or freed handle");
        self.free.push(handle);
        value
    }

    /// Number of live handles.
    pub fn len(&self) -> usize {
        self.next as usize - 1 - self.free.len()
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }
}

impl Default for HandleTable {
    fn default() -> Self {
        Self::new()
    }
}
