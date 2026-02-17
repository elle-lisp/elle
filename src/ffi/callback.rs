//! C Callbacks - Enable C code to call Elle functions
//!
//! This module provides callback wrappers that allow C libraries to call back into Elle code.
//! Callbacks are registered with metadata about their signature for validation.
//!
//! # Architecture
//!
//! - Callbacks are identified by unique IDs
//! - Callback metadata (arg types, return type) is stored in the FFI subsystem
//! - The actual Elle closure is managed separately by the VM
//! - This avoids threading issues with non-thread-safe types like Rc

use std::collections::HashMap;
use std::sync::atomic::{AtomicU32, Ordering};
use std::sync::Mutex;

use crate::ffi::types::CType;
use crate::value::Value;
use std::rc::Rc;

/// Next callback ID to assign
static NEXT_CALLBACK_ID: AtomicU32 = AtomicU32::new(1);

// Thread-local callback registry
// Maps callback IDs to their associated closures
thread_local! {
    static CALLBACK_REGISTRY: Mutex<HashMap<u32, Rc<Value>>> = Mutex::new(HashMap::new());
}

/// Information about a registered callback (thread-safe metadata)
#[derive(Clone, Debug)]
pub struct CallbackInfo {
    /// Unique ID for this callback
    pub id: u32,
    /// Argument types for validation
    pub arg_types: Vec<CType>,
    /// Return type
    pub return_type: CType,
}

impl CallbackInfo {
    /// Create new callback info
    pub fn new(id: u32, arg_types: Vec<CType>, return_type: CType) -> Self {
        CallbackInfo {
            id,
            arg_types,
            return_type,
        }
    }
}

/// Create a new callback ID and metadata
///
/// # Arguments
/// - `arg_types`: Types of arguments the callback expects
/// - `return_type`: Return type of the callback
///
/// # Returns
/// A callback ID and metadata that can be used for validation
pub fn create_callback(arg_types: Vec<CType>, return_type: CType) -> (u32, CallbackInfo) {
    let id = NEXT_CALLBACK_ID.fetch_add(1, Ordering::SeqCst);
    let info = CallbackInfo::new(id, arg_types, return_type);
    (id, info)
}

/// Create a C callback wrapper that can be passed to C code
pub struct CCallback {
    pub id: u32,
    pub arg_types: Vec<CType>,
    pub return_type: CType,
}

impl CCallback {
    /// Create a new callback wrapper
    pub fn new(id: u32, arg_types: Vec<CType>, return_type: CType) -> Self {
        CCallback {
            id,
            arg_types,
            return_type,
        }
    }

    /// Convert callback ID to a pointer that can be passed to C
    pub fn as_ptr(&self) -> *const std::ffi::c_void {
        self.id as *const std::ffi::c_void
    }

    /// Extract callback ID from a pointer returned by C
    pub fn from_ptr(ptr: *const std::ffi::c_void) -> u32 {
        ptr as usize as u32
    }
}

/// Register a callback with an Elle closure
///
/// # Arguments
/// - id: Callback ID
/// - closure: The Elle closure/function to call when callback invoked
///
/// # Returns
/// True if successful, false if callback ID already registered
pub fn register_callback(id: u32, closure: Rc<Value>) -> bool {
    CALLBACK_REGISTRY.with(|registry| {
        let mut map = registry.lock().unwrap();
        use std::collections::hash_map::Entry;
        match map.entry(id) {
            Entry::Occupied(_) => false,
            Entry::Vacant(v) => {
                v.insert(closure);
                true
            }
        }
    })
}

/// Retrieve a registered callback closure
///
/// # Arguments
/// - id: Callback ID
///
/// # Returns
/// The Elle closure if found
pub fn get_callback(id: u32) -> Option<Rc<Value>> {
    CALLBACK_REGISTRY.with(|registry| {
        let map = registry.lock().unwrap();
        map.get(&id).cloned()
    })
}

/// Unregister and cleanup a callback
///
/// # Arguments
/// - id: Callback ID
///
/// # Returns
/// True if callback was found and removed
pub fn unregister_callback(id: u32) -> bool {
    CALLBACK_REGISTRY.with(|registry| {
        let mut map = registry.lock().unwrap();
        map.remove(&id).is_some()
    })
}

/// Invoke a callback with the given arguments.
///
/// This function is called when C code invokes a callback that was registered as an Elle closure.
/// It retrieves the closure from the registry, calls it with the provided arguments,
/// and returns the result.
///
/// # Arguments
/// - id: Callback ID
/// - args: Arguments to pass to the callback (as Elle values)
///
/// # Returns
/// The result of calling the closure, or an error if callback not found
pub fn invoke_callback(id: u32, args: Vec<Value>) -> Result<Value, String> {
    // Retrieve the closure from the registry
    let _closure =
        get_callback(id).ok_or_else(|| format!("Callback with ID {} not registered", id))?;

    // For now, this is a placeholder that returns a simple value
    // In full implementation, this would need to:
    // 1. Get the current VM context from thread-local storage
    // 2. Call the closure with the provided arguments
    // 3. Handle any exceptions that occur
    // 4. Marshal the return value appropriately
    //
    // Since we don't have direct VM access here (to avoid circular dependencies),
    // this functionality would be called from ffi_primitives or vm.rs with VM context

    // Simple placeholder: return the first argument if provided
    if !args.is_empty() {
        Ok(args[0])
    } else {
        Ok(Value::NIL)
    }
}

/// Check if a callback is registered
///
/// # Arguments
/// - id: Callback ID
///
/// # Returns
/// True if callback is registered, false otherwise
pub fn callback_exists(id: u32) -> bool {
    CALLBACK_REGISTRY.with(|registry| {
        let map = registry.lock().unwrap();
        map.contains_key(&id)
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_callback_creation() {
        let (id1, info1) = create_callback(vec![CType::Int], CType::Void);
        let (id2, info2) = create_callback(vec![CType::Float], CType::Int);

        assert_ne!(id1, id2);
        assert_eq!(info1.id, id1);
        assert_eq!(info2.id, id2);
        assert_eq!(info1.arg_types, vec![CType::Int]);
        assert_eq!(info2.return_type, CType::Int);
    }

    #[test]
    fn test_callback_pointer_conversion() {
        let callback = CCallback::new(12345, vec![], CType::Void);
        let ptr = callback.as_ptr();
        let id = CCallback::from_ptr(ptr);
        assert_eq!(id, 12345);
    }

    #[test]
    fn test_callback_info_clone() {
        let info = CallbackInfo::new(42, vec![CType::Int, CType::Float], CType::Double);
        let info2 = info.clone();
        assert_eq!(info.id, info2.id);
        assert_eq!(info.arg_types, info2.arg_types);
    }

    #[test]
    fn test_callback_registration() {
        let closure = Rc::new(Value::int(42));
        let id = 9999;

        // Register callback
        assert!(register_callback(id, closure.clone()));

        // Should not allow re-registration
        assert!(!register_callback(id, closure.clone()));

        // Should retrieve the closure
        let retrieved = get_callback(id);
        assert!(retrieved.is_some());
        assert_eq!(*retrieved.unwrap(), Value::int(42));

        // Cleanup
        assert!(unregister_callback(id));
        assert!(get_callback(id).is_none());
    }

    #[test]
    fn test_callback_unregister_nonexistent() {
        let id = 8888;
        assert!(!unregister_callback(id));
    }

    #[test]
    fn test_multiple_callbacks() {
        let closure1 = Rc::new(Value::int(1));
        let closure2 = Rc::new(Value::int(2));
        let closure3 = Rc::new(Value::int(3));

        assert!(register_callback(100, closure1.clone()));
        assert!(register_callback(101, closure2.clone()));
        assert!(register_callback(102, closure3.clone()));

        assert_eq!(*get_callback(100).unwrap(), Value::int(1));
        assert_eq!(*get_callback(101).unwrap(), Value::int(2));
        assert_eq!(*get_callback(102).unwrap(), Value::int(3));

        // Cleanup
        assert!(unregister_callback(100));
        assert!(unregister_callback(101));
        assert!(unregister_callback(102));
    }

    #[test]
    fn test_callback_exists() {
        let closure = Rc::new(Value::int(42));
        let id = 7777;

        // Callback doesn't exist yet
        assert!(!callback_exists(id));

        // Register it
        register_callback(id, closure);
        assert!(callback_exists(id));

        // Unregister it
        unregister_callback(id);
        assert!(!callback_exists(id));
    }

    #[test]
    fn test_invoke_callback_simple() {
        let closure = Rc::new(Value::int(99));
        let id = 6666;

        register_callback(id, closure);

        // Invoke with no args - should return nil in placeholder
        let result = invoke_callback(id, vec![]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::NIL);

        // Invoke with args - should return first arg in placeholder
        let result = invoke_callback(id, vec![Value::int(123)]);
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), Value::int(123));

        unregister_callback(id);
    }

    #[test]
    fn test_invoke_callback_nonexistent() {
        let id = 5555;
        let result = invoke_callback(id, vec![]);
        assert!(result.is_err());
        assert!(result.unwrap_err().contains("not registered"));
    }
}
