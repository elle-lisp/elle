//! Heap-allocated value types for the NaN-boxed value system.
//!
//! All non-immediate values (strings, cons cells, vectors, closures, etc.)
//! are stored on the heap and accessed through `HeapObject`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::ffi::c_void;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::value::Value;

// Re-use types from the old value system during migration
// These will be moved here once the migration is complete
pub use crate::value_old::{
    Arity, Closure, Condition, Coroutine, JitClosure, JitLambda, NativeFn, TableKey, VmAwareFn,
};

/// Cons cell for list construction using NaN-boxed values.
#[derive(Debug, Clone, PartialEq)]
pub struct Cons {
    pub first: Value,
    pub rest: Value,
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        Cons { first, rest }
    }
}

/// Discriminant for heap object types.
/// Used for fast type checking without full pattern matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(u8)]
pub enum HeapTag {
    String = 0,
    Cons = 1,
    Vector = 2,
    Table = 3,
    Struct = 4,
    Closure = 5,
    Condition = 6,
    Coroutine = 7,
    Cell = 8,
    Float = 9, // For NaN values that can't be inline
    NativeFn = 10,
    VmAwareFn = 11,
    LibHandle = 12,
    CHandle = 13,
    ThreadHandle = 14,
}

/// All heap-allocated value types.
///
/// Each variant corresponds to a type that cannot be represented inline
/// in the NaN-boxed Value. Objects are allocated on the heap and accessed
/// via pointer.
pub enum HeapObject {
    /// Immutable string
    String(Box<str>),

    /// Cons cell (list pair)
    Cons(Cons),

    /// Mutable vector
    Vector(RefCell<Vec<Value>>),

    /// Mutable table (hash map)
    Table(RefCell<BTreeMap<TableKey, Value>>),

    /// Immutable struct
    Struct(BTreeMap<TableKey, Value>),

    /// Function closure (interpreted and/or JIT-compiled)
    /// Note: Currently uses separate Closure/JitClosure; will be unified
    Closure(Rc<Closure>),

    /// JIT-compiled closure (temporary, will merge into Closure)
    JitClosure(Rc<JitClosure>),

    /// Exception/condition object
    Condition(Condition),

    /// Suspendable computation
    Coroutine(Rc<RefCell<Coroutine>>),

    /// Mutable cell for captured variables.
    /// The boolean distinguishes compiler-created cells (true, auto-unwrapped
    /// by LoadUpvalue) from user-created cells via `box` (false, not auto-unwrapped).
    Cell(RefCell<Value>, bool),

    /// Float value that couldn't be stored inline (NaN payload)
    Float(f64),

    /// Native function (Rust function)
    NativeFn(NativeFn),

    /// VM-aware native function
    VmAwareFn(VmAwareFn),

    /// FFI library handle
    LibHandle(u32),

    /// FFI C pointer handle
    CHandle(*const c_void, u32),

    /// Thread handle for concurrent execution
    ThreadHandle(ThreadHandleData),
}

/// Data for thread handles.
/// Holds the result of a spawned thread's execution.
/// Uses Arc<Mutex<>> to safely share the result across threads.
pub struct ThreadHandleData {
    pub result: Arc<Mutex<Option<Result<crate::value::SendValue, String>>>>,
}

impl HeapObject {
    /// Get the type tag for this heap object.
    #[inline]
    pub fn tag(&self) -> HeapTag {
        match self {
            HeapObject::String(_) => HeapTag::String,
            HeapObject::Cons(_) => HeapTag::Cons,
            HeapObject::Vector(_) => HeapTag::Vector,
            HeapObject::Table(_) => HeapTag::Table,
            HeapObject::Struct(_) => HeapTag::Struct,
            HeapObject::Closure(_) => HeapTag::Closure,
            HeapObject::JitClosure(_) => HeapTag::Closure, // Same tag for now
            HeapObject::Condition(_) => HeapTag::Condition,
            HeapObject::Coroutine(_) => HeapTag::Coroutine,
            HeapObject::Cell(_, _) => HeapTag::Cell,
            HeapObject::Float(_) => HeapTag::Float,
            HeapObject::NativeFn(_) => HeapTag::NativeFn,
            HeapObject::VmAwareFn(_) => HeapTag::VmAwareFn,
            HeapObject::LibHandle(_) => HeapTag::LibHandle,
            HeapObject::CHandle(_, _) => HeapTag::CHandle,
            HeapObject::ThreadHandle(_) => HeapTag::ThreadHandle,
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            HeapObject::String(_) => "string",
            HeapObject::Cons(_) => "cons",
            HeapObject::Vector(_) => "vector",
            HeapObject::Table(_) => "table",
            HeapObject::Struct(_) => "struct",
            HeapObject::Closure(_) | HeapObject::JitClosure(_) => "closure",
            HeapObject::Condition(_) => "condition",
            HeapObject::Coroutine(_) => "coroutine",
            HeapObject::Cell(_, _) => "cell",
            HeapObject::Float(_) => "float",
            HeapObject::NativeFn(_) => "native-function",
            HeapObject::VmAwareFn(_) => "vm-aware-function",
            HeapObject::LibHandle(_) => "library-handle",
            HeapObject::CHandle(_, _) => "c-handle",
            HeapObject::ThreadHandle(_) => "thread-handle",
        }
    }
}

impl std::fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeapObject::String(s) => write!(f, "\"{}\"", s),
            HeapObject::Cons(c) => write!(f, "({:?} . {:?})", c.first, c.rest),
            HeapObject::Vector(v) => {
                if let Ok(borrowed) = v.try_borrow() {
                    write!(f, "{:?}", &*borrowed)
                } else {
                    write!(f, "[<borrowed>]")
                }
            }
            HeapObject::Table(_) => write!(f, "<table>"),
            HeapObject::Struct(_) => write!(f, "<struct>"),
            HeapObject::Closure(_) => write!(f, "<closure>"),
            HeapObject::JitClosure(_) => write!(f, "<jit-closure>"),
            HeapObject::Condition(c) => write!(f, "<condition:{}>", c.exception_id),
            HeapObject::Coroutine(_) => write!(f, "<coroutine>"),
            HeapObject::Cell(_, _) => write!(f, "<cell>"),
            HeapObject::Float(n) => write!(f, "{}", n),
            HeapObject::NativeFn(_) => write!(f, "<native-fn>"),
            HeapObject::VmAwareFn(_) => write!(f, "<vm-aware-fn>"),
            HeapObject::LibHandle(id) => write!(f, "<lib-handle:{}>", id),
            HeapObject::CHandle(_, id) => write!(f, "<c-handle:{}>", id),
            HeapObject::ThreadHandle(_) => write!(f, "<thread-handle>"),
        }
    }
}

// =============================================================================
// Heap Allocation
// =============================================================================

/// Allocated heap object with reference counting.
/// This is what Value::Heap pointers actually point to.
pub type HeapRef = Rc<HeapObject>;

/// Allocate a heap object and return a Value pointing to it.
pub fn alloc(obj: HeapObject) -> Value {
    let heap_ref: HeapRef = Rc::new(obj);
    let ptr = Rc::into_raw(heap_ref) as *const ();
    Value::from_heap_ptr(ptr)
}

/// Get a reference to a heap object from a Value.
///
/// # Safety
/// The Value must be a heap pointer (is_heap() returns true).
#[inline]
pub unsafe fn deref(value: Value) -> &'static HeapObject {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    &*ptr
}

/// Clone (increment refcount) a heap value.
///
/// # Safety
/// The Value must be a heap pointer.
#[inline]
pub unsafe fn clone_heap(value: Value) {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    let rc = Rc::from_raw(ptr);
    let _ = Rc::clone(&rc);
    std::mem::forget(rc); // Don't decrement refcount
}

/// Drop (decrement refcount) a heap value.
///
/// # Safety
/// The Value must be a heap pointer.
#[inline]
pub unsafe fn drop_heap(value: Value) {
    let ptr = value.as_heap_ptr().unwrap() as *const HeapObject;
    drop(Rc::from_raw(ptr));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_string() {
        let v = alloc(HeapObject::String("hello".into()));
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::String(s) => assert_eq!(&**s, "hello"),
                _ => panic!("Expected String"),
            }
            // Clean up
            drop_heap(v);
        }
    }

    #[test]
    fn test_heap_tag() {
        let s = HeapObject::String("test".into());
        assert_eq!(s.tag(), HeapTag::String);
        assert_eq!(s.type_name(), "string");
    }
}
