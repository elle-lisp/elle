//! Primitive registry for JIT compilation
//!
//! Provides a registry of primitive functions that can be called directly
//! from JIT-compiled code via their known memory addresses.

use crate::value::{Arity, Value};
use std::collections::HashMap;

/// JIT-compatible primitive function signature.
///
/// All primitives are called through this uniform interface:
/// - args_ptr: pointer to array of Value (as i64-encoded)
/// - args_len: number of arguments
/// - Returns: i64-encoded Value (or encoded error)
pub type JitPrimitiveFn = unsafe extern "C" fn(args_ptr: *const i64, args_len: usize) -> i64;

/// Entry in the primitive registry
#[derive(Clone)]
pub struct PrimitiveEntry {
    /// The JIT-compatible function pointer
    pub func_ptr: JitPrimitiveFn,
    /// Expected arity for validation
    pub arity: Arity,
    /// Original name (for debugging)
    pub name: &'static str,
}

/// Registry of primitives available for JIT compilation
pub struct PrimitiveRegistry {
    entries: HashMap<&'static str, PrimitiveEntry>,
}

impl PrimitiveRegistry {
    pub fn new() -> Self {
        let mut registry = PrimitiveRegistry {
            entries: HashMap::new(),
        };
        registry.register_core_primitives();
        registry
    }

    /// Register a primitive function
    pub fn register(&mut self, name: &'static str, func: JitPrimitiveFn, arity: Arity) {
        self.entries.insert(
            name,
            PrimitiveEntry {
                func_ptr: func,
                arity,
                name,
            },
        );
    }

    /// Look up a primitive by name
    pub fn get(&self, name: &str) -> Option<&PrimitiveEntry> {
        self.entries.get(name)
    }

    /// Check if a name is a registered primitive
    pub fn is_primitive(&self, name: &str) -> bool {
        self.entries.contains_key(name)
    }

    /// Register all core primitives
    fn register_core_primitives(&mut self) {
        // List operations - these are the hot ones for nqueens
        self.register("first", jit_prim_first, Arity::Exact(1));
        self.register("rest", jit_prim_rest, Arity::Exact(1));
        self.register("cons", jit_prim_cons, Arity::Exact(2));
        self.register("empty?", jit_prim_empty, Arity::Exact(1));
        self.register("nil?", jit_prim_is_nil, Arity::Exact(1));
        self.register("list", jit_prim_list, Arity::AtLeast(0));
        self.register("length", jit_prim_length, Arity::Exact(1));
        self.register("append", jit_prim_append, Arity::AtLeast(0));
        self.register("reverse", jit_prim_reverse, Arity::Exact(1));

        // Arithmetic - already have intrinsics, but add for completeness
        self.register("+", jit_prim_add, Arity::AtLeast(0));
        self.register("-", jit_prim_sub, Arity::AtLeast(1));
        self.register("*", jit_prim_mul, Arity::AtLeast(0));
        self.register("/", jit_prim_div, Arity::AtLeast(1));
        self.register("abs", jit_prim_abs, Arity::Exact(1));

        // Comparisons
        self.register("=", jit_prim_eq, Arity::AtLeast(2));
        self.register("<", jit_prim_lt, Arity::AtLeast(2));
        self.register(">", jit_prim_gt, Arity::AtLeast(2));
        self.register("<=", jit_prim_le, Arity::AtLeast(2));
        self.register(">=", jit_prim_ge, Arity::AtLeast(2));

        // Logic
        self.register("not", jit_prim_not, Arity::Exact(1));

        // Type predicates
        self.register("pair?", jit_prim_is_pair, Arity::Exact(1));
        self.register("number?", jit_prim_is_number, Arity::Exact(1));
    }
}

impl Default for PrimitiveRegistry {
    fn default() -> Self {
        Self::new()
    }
}

// ============================================================================
// Value Encoding/Decoding for FFI
// ============================================================================

/// Encode a Value as an i64 for JIT interop
///
/// For Phase 1, we use a simple approach: box the value and return its pointer.
/// This is not optimal but correct and simple.
pub fn encode_value(value: Value) -> i64 {
    let boxed = Box::new(value);
    Box::into_raw(boxed) as i64
}

/// Decode an i64 back to a Value
///
/// # Safety
/// The i64 must be a valid encoded Value from encode_value
pub fn decode_value(encoded: i64) -> Value {
    if encoded == 0 {
        return Value::NIL;
    }
    unsafe {
        let ptr = encoded as *mut Value;
        *Box::from_raw(ptr)
    }
}

/// Decode an i64 as a reference to Value (for reading args)
///
/// # Safety
/// The pointer must be valid and point to a Value
pub unsafe fn decode_value_ref(encoded: i64) -> &'static Value {
    &*(encoded as *const Value)
}

// ============================================================================
// JIT Primitive Wrappers
// ============================================================================

use crate::primitives::arithmetic::{prim_abs, prim_add, prim_div, prim_mul, prim_sub};
use crate::primitives::comparison::{prim_eq, prim_ge, prim_gt, prim_le, prim_lt};
use crate::primitives::list::{
    prim_append, prim_cons, prim_empty, prim_first, prim_length, prim_list, prim_rest, prim_reverse,
};
use crate::primitives::logic::prim_not;
use crate::primitives::type_check::{prim_is_nil, prim_is_number, prim_is_pair};

/// Helper macro to generate JIT primitive wrappers
///
/// # Safety
/// The generated function expects args_ptr to be a valid pointer to an array of args_len i64 values.
/// The JIT compiler is responsible for ensuring this invariant.
macro_rules! jit_primitive_wrapper {
    ($name:ident, $prim:path) => {
        /// JIT wrapper for a primitive function.
        ///
        /// # Safety
        /// The caller must ensure that args_ptr is a valid pointer to an array of args_len i64 values.
        pub unsafe extern "C" fn $name(args_ptr: *const i64, args_len: usize) -> i64 {
            // Convert args from i64 pointers to Value slice
            let args: Vec<Value> = (0..args_len)
                .map(|i| decode_value(*args_ptr.add(i)))
                .collect();

            // Call the actual primitive
            match $prim(&args) {
                Ok(result) => encode_value(result),
                Err(_) => encode_value(Value::NIL), // TODO: proper error handling
            }
        }
    };
}

// Generate wrappers for all primitives
jit_primitive_wrapper!(jit_prim_first, prim_first);
jit_primitive_wrapper!(jit_prim_rest, prim_rest);
jit_primitive_wrapper!(jit_prim_cons, prim_cons);
jit_primitive_wrapper!(jit_prim_empty, prim_empty);
jit_primitive_wrapper!(jit_prim_is_nil, prim_is_nil);
jit_primitive_wrapper!(jit_prim_list, prim_list);
jit_primitive_wrapper!(jit_prim_length, prim_length);
jit_primitive_wrapper!(jit_prim_append, prim_append);
jit_primitive_wrapper!(jit_prim_reverse, prim_reverse);
jit_primitive_wrapper!(jit_prim_add, prim_add);
jit_primitive_wrapper!(jit_prim_sub, prim_sub);
jit_primitive_wrapper!(jit_prim_mul, prim_mul);
jit_primitive_wrapper!(jit_prim_div, prim_div);
jit_primitive_wrapper!(jit_prim_abs, prim_abs);
jit_primitive_wrapper!(jit_prim_eq, prim_eq);
jit_primitive_wrapper!(jit_prim_lt, prim_lt);
jit_primitive_wrapper!(jit_prim_gt, prim_gt);
jit_primitive_wrapper!(jit_prim_le, prim_le);
jit_primitive_wrapper!(jit_prim_ge, prim_ge);
jit_primitive_wrapper!(jit_prim_not, prim_not);
jit_primitive_wrapper!(jit_prim_is_pair, prim_is_pair);
jit_primitive_wrapper!(jit_prim_is_number, prim_is_number);

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_primitive_registry_creation() {
        let registry = PrimitiveRegistry::new();
        assert!(registry.is_primitive("first"));
        assert!(registry.is_primitive("rest"));
        assert!(registry.is_primitive("cons"));
    }

    #[test]
    fn test_primitive_registry_lookup() {
        let registry = PrimitiveRegistry::new();
        let entry = registry.get("first");
        assert!(entry.is_some());
        assert_eq!(entry.unwrap().name, "first");
    }

    #[test]
    fn test_primitive_registry_unknown() {
        let registry = PrimitiveRegistry::new();
        assert!(!registry.is_primitive("unknown-op"));
    }

    #[test]
    fn test_value_encoding_decoding() {
        let original = Value::int(42);
        let encoded = encode_value(original);
        let decoded = decode_value(encoded);
        assert_eq!(original, decoded);
    }

    #[test]
    fn test_value_encoding_nil() {
        let original = Value::NIL;
        let encoded = encode_value(original);
        let decoded = decode_value(encoded);
        assert_eq!(original, decoded);
    }
}
