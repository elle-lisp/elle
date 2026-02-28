//! Heap-allocated value types for the NaN-boxed value system.
//!
//! All non-immediate values (strings, cons cells, vectors, closures, etc.)
//! are stored on the heap and accessed through `HeapObject`.

use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use crate::syntax::Syntax;
use crate::value::fiber::FiberHandle;
use crate::value::types::SymbolId;
use crate::value::Value;

// Re-export types for convenience
pub use crate::value::closure::Closure;
pub use crate::value::types::{Arity, NativeFn, TableKey};

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
    Array = 2,
    Table = 3,
    Struct = 4,
    Closure = 5,
    Syntax = 6,
    Tuple = 7,
    Cell = 8,
    Float = 9, // For NaN values that can't be inline
    NativeFn = 10,
    LibHandle = 12,
    ThreadHandle = 14,
    Fiber = 16,
    Binding = 17,
    FFISignature = 18,
    FFIType = 19,
    ManagedPointer = 20,
    Buffer = 21,
    Bytes = 22,
    Blob = 23,
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

    /// Mutable array
    Array(RefCell<Vec<Value>>),

    /// Mutable table (hash map)
    Table(RefCell<BTreeMap<TableKey, Value>>),

    /// Immutable struct
    Struct(BTreeMap<TableKey, Value>),

    /// Function closure (interpreted)
    Closure(Rc<Closure>),

    /// Immutable tuple (fixed-length sequence)
    Tuple(Vec<Value>),

    /// Mutable buffer (byte sequence)
    Buffer(RefCell<Vec<u8>>),

    /// Immutable byte sequence (binary data)
    Bytes(Vec<u8>),

    /// Mutable byte sequence (binary data workspace)
    Blob(RefCell<Vec<u8>>),

    /// Mutable cell for captured variables.
    /// The boolean distinguishes compiler-created cells (true, auto-unwrapped
    /// by LoadUpvalue) from user-created cells via `box` (false, not auto-unwrapped).
    Cell(RefCell<Value>, bool),

    /// Float value that couldn't be stored inline (NaN payload)
    Float(f64),

    /// Native function (Rust function)
    NativeFn(NativeFn),

    /// FFI library handle
    LibHandle(u32),

    /// Thread handle for concurrent execution
    ThreadHandle(ThreadHandle),

    /// Fiber: independent execution context with its own stack and frames
    Fiber(FiberHandle),

    /// Syntax object: preserves scope sets through the Value round-trip
    /// during macro expansion. This is the only HeapObject variant that
    /// references compile-time types — an intentional coupling required
    /// for first-class syntax objects in hygienic macros.
    Syntax(Rc<Syntax>),

    /// Compile-time binding metadata. Mutable during analysis (the analyzer
    /// discovers captures and mutations after creating the binding), read-only
    /// during lowering. Never appears at runtime — the VM never sees this type.
    Binding(RefCell<BindingInner>),

    /// Reified FFI function signature with optional cached CIF.
    /// The CIF is lazily prepared on first use and reused thereafter.
    FFISignature(
        crate::ffi::types::Signature,
        RefCell<Option<libffi::middle::Cif>>,
    ),

    /// Reified FFI compound type descriptor (struct or array layout)
    FFIType(crate::ffi::types::TypeDesc),

    /// Managed FFI pointer with lifecycle tracking.
    /// `Some(addr)` = live, `None` = freed. Only for ffi/malloc'd memory.
    ManagedPointer(std::cell::Cell<Option<usize>>),
}

/// Internal binding metadata, heap-allocated behind the Value pointer.
#[derive(Debug)]
pub struct BindingInner {
    /// Original symbol name (for error messages and global lookup)
    pub name: SymbolId,
    /// Where this binding lives
    pub scope: BindingScope,
    /// Whether this binding has been mutated via set!
    pub is_mutated: bool,
    /// Whether this binding is captured by a nested closure
    pub is_captured: bool,
    /// Whether this binding is immutable (def)
    pub is_immutable: bool,
}

/// Where a binding lives at runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingScope {
    /// Lambda parameter
    Parameter,
    /// Local variable (let-bound, define inside function)
    Local,
    /// Global/top-level definition
    Global,
}

/// Thread handle for concurrent execution.
///
/// Holds the result of a spawned thread's execution.
/// Uses `Arc<Mutex<>>` to safely share the result across threads.
#[derive(Clone)]
pub struct ThreadHandle {
    /// The result of the spawned thread execution.
    /// The `Result` is wrapped in `SendValue` to make it Send.
    pub result: Arc<Mutex<Option<Result<crate::value::SendValue, String>>>>,
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

impl HeapObject {
    /// Get the type tag for this heap object.
    #[inline]
    pub fn tag(&self) -> HeapTag {
        match self {
            HeapObject::String(_) => HeapTag::String,
            HeapObject::Cons(_) => HeapTag::Cons,
            HeapObject::Array(_) => HeapTag::Array,
            HeapObject::Table(_) => HeapTag::Table,
            HeapObject::Struct(_) => HeapTag::Struct,
            HeapObject::Closure(_) => HeapTag::Closure,
            HeapObject::Tuple(_) => HeapTag::Tuple,
            HeapObject::Buffer(_) => HeapTag::Buffer,
            HeapObject::Bytes(_) => HeapTag::Bytes,
            HeapObject::Blob(_) => HeapTag::Blob,
            HeapObject::Cell(_, _) => HeapTag::Cell,
            HeapObject::Float(_) => HeapTag::Float,
            HeapObject::NativeFn(_) => HeapTag::NativeFn,
            HeapObject::LibHandle(_) => HeapTag::LibHandle,
            HeapObject::ThreadHandle(_) => HeapTag::ThreadHandle,
            HeapObject::Fiber(_) => HeapTag::Fiber,
            HeapObject::Syntax(_) => HeapTag::Syntax,
            HeapObject::Binding(_) => HeapTag::Binding,
            HeapObject::FFISignature(_, _) => HeapTag::FFISignature,
            HeapObject::FFIType(_) => HeapTag::FFIType,
            HeapObject::ManagedPointer(_) => HeapTag::ManagedPointer,
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            HeapObject::String(_) => "string",
            HeapObject::Cons(_) => "list",
            HeapObject::Array(_) => "array",
            HeapObject::Table(_) => "table",
            HeapObject::Struct(_) => "struct",
            HeapObject::Closure(_) => "closure",
            HeapObject::Tuple(_) => "tuple",
            HeapObject::Buffer(_) => "buffer",
            HeapObject::Bytes(_) => "bytes",
            HeapObject::Blob(_) => "blob",
            HeapObject::Cell(_, _) => "cell",
            HeapObject::Float(_) => "float",
            HeapObject::NativeFn(_) => "native-function",
            HeapObject::LibHandle(_) => "library-handle",
            HeapObject::ThreadHandle(_) => "thread-handle",
            HeapObject::Fiber(_) => "fiber",
            HeapObject::Syntax(_) => "syntax",
            HeapObject::Binding(_) => "binding",
            HeapObject::FFISignature(_, _) => "ffi-signature",
            HeapObject::FFIType(_) => "ffi-type",
            HeapObject::ManagedPointer(_) => "pointer",
        }
    }
}

impl std::fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeapObject::String(s) => write!(f, "\"{}\"", s),
            HeapObject::Cons(c) => write!(f, "({:?} . {:?})", c.first, c.rest),
            HeapObject::Array(v) => {
                if let Ok(borrowed) = v.try_borrow() {
                    write!(f, "{:?}", &*borrowed)
                } else {
                    write!(f, "[<borrowed>]")
                }
            }
            HeapObject::Table(_) => write!(f, "<table>"),
            HeapObject::Struct(_) => write!(f, "<struct>"),
            HeapObject::Closure(_) => write!(f, "<closure>"),
            HeapObject::Tuple(elems) => {
                write!(f, "[")?;
                for (i, v) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", v)?;
                }
                write!(f, "]")
            }
            HeapObject::Buffer(v) => {
                if let Ok(borrowed) = v.try_borrow() {
                    write!(f, "@\"{}\"", String::from_utf8_lossy(&borrowed))
                } else {
                    write!(f, "@\"<borrowed>\"")
                }
            }
            HeapObject::Bytes(b) => {
                write!(f, "#bytes[")?;
                for (i, byte) in b.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:02x}", byte)?;
                }
                write!(f, "]")
            }
            HeapObject::Blob(b) => {
                if let Ok(borrowed) = b.try_borrow() {
                    write!(f, "#blob[")?;
                    for (i, byte) in borrowed.iter().enumerate() {
                        if i > 0 {
                            write!(f, " ")?;
                        }
                        write!(f, "{:02x}", byte)?;
                    }
                    write!(f, "]")
                } else {
                    write!(f, "#blob[<borrowed>]")
                }
            }
            HeapObject::Cell(_, _) => write!(f, "<cell>"),
            HeapObject::Float(n) => write!(f, "{}", n),
            HeapObject::NativeFn(_) => write!(f, "<native-fn>"),
            HeapObject::LibHandle(id) => write!(f, "<lib-handle:{}>", id),
            HeapObject::ThreadHandle(_) => write!(f, "<thread-handle>"),
            HeapObject::Fiber(handle) => match handle.try_with(|fib| fib.status.as_str()) {
                Some(status) => write!(f, "<fiber:{}>", status),
                None => write!(f, "<fiber:taken>"),
            },
            HeapObject::Syntax(s) => write!(f, "#<syntax:{}>", s),
            HeapObject::Binding(_) => write!(f, "#<binding>"),
            HeapObject::FFISignature(_, _) => write!(f, "<ffi-signature>"),
            HeapObject::FFIType(desc) => write!(f, "<ffi-type:{:?}>", desc),
            HeapObject::ManagedPointer(cell) => match cell.get() {
                Some(addr) => write!(f, "<managed-pointer 0x{:x}>", addr),
                None => write!(f, "<freed-pointer>"),
            },
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
