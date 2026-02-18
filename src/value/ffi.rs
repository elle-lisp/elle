//! FFI types for the Elle runtime
//!
//! Types for interacting with foreign (C) code via libloading.

use crate::value::SendValue;
use std::ffi::c_void;
use std::sync::{Arc, Mutex};

/// FFI library handle
///
/// Wraps a handle ID for a loaded dynamic library.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct LibHandle(pub u32);

/// FFI C object handle (opaque pointer to C data)
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct CHandle {
    /// Raw C pointer
    pub ptr: *const c_void,
    /// Unique ID for this handle
    pub id: u32,
}

impl CHandle {
    /// Create a new C handle
    pub fn new(ptr: *const c_void, id: u32) -> Self {
        CHandle { ptr, id }
    }
}

/// Thread handle for concurrent execution.
///
/// Holds the result of a spawned thread's execution.
/// Uses `Arc<Mutex<>>` to safely share the result across threads.
#[derive(Clone)]
pub struct ThreadHandle {
    /// The result of the spawned thread execution.
    /// The `Result` is wrapped in `SendValue` to make it Send.
    pub result: Arc<Mutex<Option<Result<SendValue, String>>>>,
}

impl ThreadHandle {
    /// Create a new thread handle with no result yet
    pub fn new() -> Self {
        ThreadHandle {
            result: Arc::new(Mutex::new(None)),
        }
    }
}

impl Default for ThreadHandle {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for ThreadHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ThreadHandle")
    }
}

impl PartialEq for ThreadHandle {
    fn eq(&self, _other: &Self) -> bool {
        false // Thread handles are never equal
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_lib_handle() {
        let h1 = LibHandle(1);
        let h2 = LibHandle(1);
        let h3 = LibHandle(2);

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_c_handle() {
        let ptr = std::ptr::null();
        let h1 = CHandle::new(ptr, 1);
        let h2 = CHandle::new(ptr, 1);
        let h3 = CHandle::new(ptr, 2);

        assert_eq!(h1, h2);
        assert_ne!(h1, h3);
    }

    #[test]
    fn test_thread_handle_not_equal() {
        let h1 = ThreadHandle::new();
        let h2 = ThreadHandle::new();

        // Thread handles are never equal
        assert_ne!(h1, h2);
    }
}
