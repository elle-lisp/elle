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
use std::collections::BTreeMap;

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

    /// Deep copy of tuples (immutable fixed-length sequences)
    Tuple(Vec<SendValue>),

    /// Deep copy of mutable cells (if contents are sendable)
    /// The bool indicates if it's a local cell (auto-unwrapped) or user cell
    Cell(Box<SendValue>, bool),

    /// Float values that couldn't be stored inline
    Float(f64),
}

impl SendValue {
    /// Convert a Value to SendValue by deep-copying heap data.
    ///
    /// Returns Err if the value contains non-sendable data (mutable tables,
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

        // String values
        if let Some(s) = value.as_string() {
            return Ok(SendValue::String(s.to_string()));
        }

        // Heap values need deep copying
        if !value.is_heap() {
            return Ok(SendValue::Immediate(value));
        }

        match unsafe { deref(value) } {
            // Strings are immutable and safe
            HeapObject::String(s) => Ok(SendValue::String(s.to_string())),

            // Cons cells - deep copy both first and rest
            HeapObject::Cons(cons) => {
                let first = SendValue::from_value(cons.first)?;
                let rest = SendValue::from_value(cons.rest)?;
                Ok(SendValue::Cons(Box::new(first), Box::new(rest)))
            }

            // Arrays - deep copy all elements
            HeapObject::Array(vec_ref) => {
                let borrowed = vec_ref
                    .try_borrow()
                    .map_err(|_| "Cannot borrow array for sending".to_string())?;
                let copied: Result<Vec<SendValue>, String> =
                    borrowed.iter().map(|v| SendValue::from_value(*v)).collect();
                Ok(SendValue::Array(copied?))
            }

            // Structs - deep copy all values
            HeapObject::Struct(s) => {
                let mut copied = BTreeMap::new();
                for (k, v) in s.iter() {
                    copied.insert(k.clone(), SendValue::from_value(*v)?);
                }
                Ok(SendValue::Struct(copied))
            }

            // Tuples - deep copy all elements
            HeapObject::Tuple(elems) => {
                let copied: Result<Vec<SendValue>, String> =
                    elems.iter().map(|v| SendValue::from_value(*v)).collect();
                Ok(SendValue::Tuple(copied?))
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

            // Unsafe: mutable tables
            HeapObject::Table(_) => Err("Cannot send mutable table".to_string()),

            // Unsafe: closures (contain function pointers and mutable state)
            HeapObject::Closure(_) => Err("Cannot send closure directly".to_string()),

            // Unsafe: native functions (contain function pointers)
            HeapObject::NativeFn(_) => Err("Cannot send native function".to_string()),

            // Unsafe: FFI handles
            HeapObject::LibHandle(_) => Err("Cannot send library handle".to_string()),
            HeapObject::CHandle(_, _) => Err("Cannot send C handle".to_string()),

            // Unsafe: thread handles
            HeapObject::ThreadHandle(_) => Err("Cannot send thread handle".to_string()),

            // Unsafe: fibers (contain execution state with closures)
            HeapObject::Fiber(_) => Err("Cannot send fiber".to_string()),

            // Unsafe: syntax objects (contain Rc)
            HeapObject::Syntax(_) => Err("Cannot send syntax object".to_string()),
        }
    }

    /// Convert SendValue back into a Value by reconstructing heap objects.
    pub fn into_value(self) -> Value {
        match self {
            SendValue::Immediate(v) => v,
            SendValue::Keyword(name) => Value::keyword(&name),
            SendValue::String(s) => {
                // Use alloc directly to avoid thread-local interner issues
                let boxed: Box<str> = s.into();
                alloc(HeapObject::String(boxed))
            }
            SendValue::Cons(first, rest) => {
                let first_val = first.into_value();
                let rest_val = rest.into_value();
                let cons = Cons::new(first_val, rest_val);
                alloc(HeapObject::Cons(cons))
            }
            SendValue::Array(items) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::Array(std::cell::RefCell::new(values)))
            }
            SendValue::Struct(map) => {
                let values: BTreeMap<_, _> = map
                    .into_iter()
                    .map(|(k, sv)| (k, sv.into_value()))
                    .collect();
                alloc(HeapObject::Struct(values))
            }
            SendValue::Tuple(items) => {
                let values: Vec<Value> = items.into_iter().map(|sv| sv.into_value()).collect();
                alloc(HeapObject::Tuple(values))
            }
            SendValue::Cell(contents, is_local) => {
                let val = contents.into_value();
                // Preserve the cell type (local vs user) across thread boundary
                alloc(HeapObject::Cell(std::cell::RefCell::new(val), is_local))
            }
            SendValue::Float(f) => alloc(HeapObject::Float(f)),
        }
    }
}

// SAFETY: SendValue is safe to send because it owns all its data
unsafe impl Send for SendValue {}
unsafe impl Sync for SendValue {}
