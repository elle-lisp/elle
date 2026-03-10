//! SendValue wrapper for thread-safe value transmission
//!
//! This module provides SendValue, a wrapper around Value that implements Send
//! by deep-copying heap values instead of sharing raw pointers.
//!
//! The problem with raw Value copies: NaN-boxed Value contains raw pointers to Rc
//! heap objects. When sent to another thread, the original Rc may drop and free the
//! heap object while the thread still holds a raw pointer to it.
//!
//! The solution: SendValue stores owned copies of heap data, not raw pointers.

use super::heap::{alloc, deref, Cons, HeapObject};
use super::repr::Value;
use std::collections::{BTreeMap, BTreeSet};

/// A thread-safe wrapper around Value that deep-copies heap data.
///
/// For immediate values (nil, bool, int, float, symbol), SendValue stores
/// them directly. Keywords carry their name for cross-thread re-interning.
/// For heap values, SendValue stores owned copies of the heap data, ensuring
/// the data remains valid even if the original Rc is dropped.
#[derive(Clone)]
pub enum SendValue {
    /// Immediate values that don't need copying
    Immediate(Value),

    /// Keyword with name for cross-thread re-interning
    Keyword(String),

    /// Owned string copy
    String(String),

    /// Deep copy of cons cells
    Cons(Box<SendValue>, Box<SendValue>),

    /// Deep copy of arrays
    Array(Vec<SendValue>),

    /// Deep copy of structs (immutable maps)
    Struct(BTreeMap<crate::value::heap::TableKey, SendValue>),

    /// Deep copy of arrays (immutable fixed-length sequences)
    Tuple(Vec<SendValue>),

    /// Deep copy of @strings (mutable byte sequences)
    Buffer(Vec<u8>),

    /// Deep copy of @bytes (immutable binary data)
    Bytes(Vec<u8>),

    /// Deep copy of @bytes (mutable binary data)
    Blob(Vec<u8>),

    /// Deep copy of mutable cells (if contents are sendable)
    /// The bool indicates if it's a local cell (auto-unwrapped) or user cell
    Cell(Box<SendValue>, bool),

    /// Float values that couldn't be stored inline
    Float(f64),

    /// Deep copy of FFI type descriptor (pure data, no Rc)
    FFIType(crate::ffi::types::TypeDesc),

    /// Deep copy of immutable sets
    LSet(Vec<SendValue>),

    /// Deep copy of mutable sets
    LSetMut(Vec<SendValue>),

    /// Native function pointer (inherently Send + Sync)
    NativeFn(crate::value::types::NativeFn),
}

impl SendValue {
    /// Convert a Value to SendValue by deep-copying heap data.
    ///
    /// Returns Err if the value contains non-sendable data (mutable @structs,
    /// native functions, FFI handles, etc.).
    pub fn from_value(value: Value) -> Result<Self, String> {
        // Keywords carry their name for cross-thread re-interning
        if let Some(name) = value.as_keyword_name() {
            return Ok(SendValue::Keyword(name.to_string()));
        }

        // Immediate values are always safe
        if value.is_nil()
            || value.is_bool()
            || value.is_int()
            || value.is_float()
            || value.is_symbol()
        {
            return Ok(SendValue::Immediate(value));
        }

        // String values (SSO or heap)
        if let Some(s) = value.with_string(|s| s.to_string()) {
            return Ok(SendValue::String(s));
        }

        // Heap values need deep copying
        if !value.is_heap() {
            return Ok(SendValue::Immediate(value));
        }

        match unsafe { deref(value) } {
            // Strings are immutable and safe
            HeapObject::LString(s) => Ok(SendValue::String(s.to_string())),

            // Cons cells - deep copy both first and rest
            HeapObject::Cons(cons) => {
                let first = SendValue::from_value(cons.first)?;
                let rest = SendValue::from_value(cons.rest)?;
                Ok(SendValue::Cons(Box::new(first), Box::new(rest)))
            }

            // Arrays - deep copy all elements
            HeapObject::LArrayMut(vec_ref) => {
                let borrowed = vec_ref
                    .try_borrow()
                    .map_err(|_| "Cannot borrow array for sending".to_string())?;
                let copied: Result<Vec<SendValue>, String> =
                    borrowed.iter().map(|v| SendValue::from_value(*v)).collect();
                Ok(SendValue::Array(copied?))
            }

            // Structs - deep copy all values
            HeapObject::LStruct(s) => {
                let mut copied = BTreeMap::new();
                for (k, v) in s.iter() {
                    if !k.is_sendable() {
                        return Err("Cannot send struct with identity keys".to_string());
                    }
                    copied.insert(k.clone(), SendValue::from_value(*v)?);
                }
                Ok(SendValue::Struct(copied))
            }

            // Tuples - deep copy all elements
            HeapObject::LArray(elems) => {
                let copied: Result<Vec<SendValue>, String> =
                    elems.iter().map(|v| SendValue::from_value(*v)).collect();
                Ok(SendValue::Tuple(copied?))
            }

            // Buffers - deep copy the bytes
            HeapObject::LStringMut(buf_ref) => {
                let borrowed = buf_ref
                    .try_borrow()
                    .map_err(|_| "Cannot borrow @string for sending".to_string())?;
                Ok(SendValue::Buffer(borrowed.clone()))
            }

            // Cells - deep copy the contents if sendable
            HeapObject::Cell(cell_ref, is_local) => {
                let borrowed = cell_ref
                    .try_borrow()
                    .map_err(|_| "Cannot borrow cell for sending".to_string())?;
                let contents = SendValue::from_value(*borrowed)?;
                Ok(SendValue::Cell(Box::new(contents), *is_local))
            }

            // Float values that couldn't be stored inline
            HeapObject::Float(f) => Ok(SendValue::Float(*f)),

            // Unsafe: mutable @structs
            HeapObject::LStructMut(_) => Err("Cannot send mutable @struct".to_string()),

            // Unsafe: closures (contain function pointers and mutable state)
            HeapObject::Closure(_) => Err("Cannot send closure directly".to_string()),

            // Native function pointers are inherently Send + Sync
            HeapObject::NativeFn(f) => Ok(SendValue::NativeFn(*f)),

            // Unsafe: FFI handles
            HeapObject::LibHandle(_) => Err("Cannot send library handle".to_string()),

            // Unsafe: thread handles
            HeapObject::ThreadHandle(_) => Err("Cannot send thread handle".to_string()),

            // Unsafe: fibers (contain execution state with closures)
            HeapObject::Fiber(_) => Err("Cannot send fiber".to_string()),

            // Unsafe: syntax objects (contain Rc)
            HeapObject::Syntax(_) => Err("Cannot send syntax object".to_string()),

            // Unsafe: bindings (compile-time only)
            HeapObject::Binding(_) => Err("Cannot send binding".to_string()),

            // Unsafe: FFI signatures (contain non-Send types like Cif)
            HeapObject::FFISignature(_, _) => Err("Cannot send FFI signature".to_string()),

            // Unsafe: managed pointers (lifecycle state is not thread-safe with Cell)
            HeapObject::ManagedPointer(_) => Err("Cannot send managed pointer".to_string()),

            // Unsafe: external objects (contain Rc<dyn Any>, not thread-safe)
            HeapObject::External(_) => Err("Cannot send external object".to_string()),

            // Unsafe: parameters (fiber-local state)
            HeapObject::Parameter { .. } => Err("Cannot send parameter".to_string()),

            // FFI type descriptors are pure data — safe to send
            HeapObject::FFIType(desc) => Ok(SendValue::FFIType(desc.clone())),

            // Bytes - immutable and safe to send
            HeapObject::LBytes(b) => Ok(SendValue::Bytes(b.clone())),

            // @bytes - deep copy the bytes
            HeapObject::LBytesMut(blob_ref) => {
                let borrowed = blob_ref
                    .try_borrow()
                    .map_err(|_| "Cannot borrow @bytes for sending".to_string())?;
                Ok(SendValue::Blob(borrowed.clone()))
            }

            // Sets (immutable) - deep copy all elements
            HeapObject::LSet(s) => {
                let copied: Result<Vec<SendValue>, String> =
                    s.iter().map(|v| SendValue::from_value(*v)).collect();
                Ok(SendValue::LSet(copied?))
            }

            // Sets (mutable) - deep copy all elements
            HeapObject::LSetMut(s_ref) => {
                let borrowed = s_ref
                    .try_borrow()
                    .map_err(|_| "Cannot borrow mutable set for sending".to_string())?;
                let copied: Result<Vec<SendValue>, String> =
                    borrowed.iter().map(|v| SendValue::from_value(*v)).collect();
                Ok(SendValue::LSetMut(copied?))
            }
        }
    }

    /// Convert SendValue back into a Value by reconstructing heap objects.
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Immediate(v) => v,
            SendValue::Keyword(name) => Value::keyword(&name),
            SendValue::String(s) => Value::string_no_intern(s),
            SendValue::Cons(first, rest) => {
                let first_val = first.into_value();
                let rest_val = rest.into_value();
                let cons = Cons::new(first_val, rest_val);
                alloc(HeapObject::Cons(cons))
            }
            SendValue::Array(items) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LArrayMut(std::cell::RefCell::new(values)))
            }
            SendValue::Struct(map) => {
                let values: BTreeMap<_, _> = map
                    .into_iter()
                    .map(|(k, sv)| (k, sv.into_value()))
                    .collect();
                alloc(HeapObject::LStruct(values))
            }
            SendValue::Tuple(items) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LArray(values))
            }
            SendValue::Buffer(bytes) => {
                alloc(HeapObject::LStringMut(std::cell::RefCell::new(bytes)))
            }
            SendValue::Bytes(bytes) => alloc(HeapObject::LBytes(bytes)),
            SendValue::Blob(bytes) => alloc(HeapObject::LBytesMut(std::cell::RefCell::new(bytes))),
            SendValue::Cell(contents, is_local) => {
                let val = contents.into_value();
                // Preserve the cell type (local vs user) across thread boundary
                alloc(HeapObject::Cell(std::cell::RefCell::new(val), is_local))
            }
            SendValue::Float(f) => alloc(HeapObject::Float(f)),
            SendValue::FFIType(desc) => alloc(HeapObject::FFIType(desc)),
            SendValue::LSet(items) => {
                let set: BTreeSet<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LSet(set))
            }
            SendValue::LSetMut(items) => {
                let set: BTreeSet<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::LSetMut(std::cell::RefCell::new(set)))
            }
            SendValue::NativeFn(f) => Value::native_fn(f),
        }
    }
}

// SAFETY: SendValue is safe to send because it owns all its data
unsafe impl Send for SendValue {}
unsafe impl Sync for SendValue {}
