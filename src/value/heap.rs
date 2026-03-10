//! Heap-allocated value types for the NaN-boxed value system.
//!
//! All non-immediate values (strings, cons cells, vectors, closures, etc.)
//! are stored on the heap and accessed through `HeapObject`.

use std::any::Any;
use std::cell::RefCell;
use std::collections::{BTreeMap, BTreeSet};
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
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Cons {
    pub first: Value,
    pub rest: Value,
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        Cons { first, rest }
    }
}

impl std::hash::Hash for Cons {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.first.hash(state);
        self.rest.hash(state);
    }
}

impl PartialOrd for Cons {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Cons {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.first
            .cmp(&other.first)
            .then_with(|| self.rest.cmp(&other.rest))
    }
}

/// Discriminant for heap object types.
/// Used for fast type checking without full pattern matching.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
#[repr(u8)]
pub enum HeapTag {
    LString = 0,
    Cons = 1,
    LArrayMut = 2,
    LStructMut = 3,
    LStruct = 4,
    Closure = 5,
    Syntax = 6,
    LArray = 7,
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
    LStringMut = 21,
    LBytes = 22,
    LBytesMut = 23,
    External = 24,
    Parameter = 25,
    LSet = 26,
    LSetMut = 27,
}

/// All heap-allocated value types.
///
/// Each variant corresponds to a type that cannot be represented inline
/// in the NaN-boxed Value. Objects are allocated on the heap and accessed
/// via pointer.
pub enum HeapObject {
    /// Immutable string
    LString(Box<str>),

    /// Cons cell (list pair)
    Cons(Cons),

    /// Mutable array
    LArrayMut(RefCell<Vec<Value>>),

    /// Mutable struct (hash map)
    LStructMut(RefCell<BTreeMap<TableKey, Value>>),

    /// Immutable struct
    LStruct(BTreeMap<TableKey, Value>),

    /// Function closure (interpreted)
    Closure(Rc<Closure>),

    /// Immutable array (fixed-length sequence)
    LArray(Vec<Value>),

    /// Mutable buffer (byte sequence)
    LStringMut(RefCell<Vec<u8>>),

    /// Immutable byte sequence (binary data)
    LBytes(Vec<u8>),

    /// Mutable byte sequence (binary data workspace)
    LBytesMut(RefCell<Vec<u8>>),

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

    /// Opaque external object from a plugin.
    /// Holds an arbitrary Rust value with a type name for Elle-side identity.
    External(ExternalObject),

    /// Dynamic parameter (Racket-style). Each parameter has a unique id
    /// (for lookup in the fiber's param_frames stack) and a default value
    /// (returned when no parameterize binding is active).
    Parameter { id: u32, default: Value },

    /// Immutable set (BTreeSet, no RefCell)
    LSet(BTreeSet<Value>),

    /// Mutable set (BTreeSet wrapped in RefCell)
    LSetMut(RefCell<BTreeSet<Value>>),
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
    /// Whether this binding was pre-created before its initializer runs
    /// (begin pass 1, letrec pass 1). Pre-bound immutable locals still
    /// need cells because they may be captured before initialization
    /// (self-recursion, forward references).
    pub is_prebound: bool,
}

/// Where a binding lives at runtime
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BindingScope {
    /// Lambda parameter
    Parameter,
    /// Local variable (let-bound, define inside function)
    Local,
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

/// Opaque external object for plugin-provided types.
/// Holds a type name (for Elle-side identity) and an arbitrary Rust value.
pub struct ExternalObject {
    pub type_name: &'static str,
    pub data: Rc<dyn Any>,
}

impl HeapObject {
    /// Get the type tag for this heap object.
    #[inline]
    pub fn tag(&self) -> HeapTag {
        match self {
            HeapObject::LString(_) => HeapTag::LString,
            HeapObject::Cons(_) => HeapTag::Cons,
            HeapObject::LArrayMut(_) => HeapTag::LArrayMut,
            HeapObject::LStructMut(_) => HeapTag::LStructMut,
            HeapObject::LStruct(_) => HeapTag::LStruct,
            HeapObject::Closure(_) => HeapTag::Closure,
            HeapObject::LArray(_) => HeapTag::LArray,
            HeapObject::LStringMut(_) => HeapTag::LStringMut,
            HeapObject::LBytes(_) => HeapTag::LBytes,
            HeapObject::LBytesMut(_) => HeapTag::LBytesMut,
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
            HeapObject::External(_) => HeapTag::External,
            HeapObject::Parameter { .. } => HeapTag::Parameter,
            HeapObject::LSet(_) => HeapTag::LSet,
            HeapObject::LSetMut(_) => HeapTag::LSetMut,
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            HeapObject::LString(_) => "string",
            HeapObject::Cons(_) => "list",
            HeapObject::LArrayMut(_) => "@array",
            HeapObject::LStructMut(_) => "@struct",
            HeapObject::LStruct(_) => "struct",
            HeapObject::Closure(_) => "closure",
            HeapObject::LArray(_) => "array",
            HeapObject::LStringMut(_) => "@string",
            HeapObject::LBytes(_) => "bytes",
            HeapObject::LBytesMut(_) => "@bytes",
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
            HeapObject::External(ext) => ext.type_name,
            HeapObject::Parameter { .. } => "parameter",
            HeapObject::LSet(_) => "set",
            HeapObject::LSetMut(_) => "@set",
        }
    }
}

impl std::fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeapObject::LString(s) => write!(f, "\"{}\"", s),
            HeapObject::Cons(c) => write!(f, "({:?} . {:?})", c.first, c.rest),
            HeapObject::LArrayMut(v) => {
                if let Ok(borrowed) = v.try_borrow() {
                    write!(f, "{:?}", &*borrowed)
                } else {
                    write!(f, "[<borrowed>]")
                }
            }
            HeapObject::LStructMut(_) => write!(f, "<@struct>"),
            HeapObject::LStruct(_) => write!(f, "<struct>"),
            HeapObject::Closure(_) => write!(f, "<closure>"),
            HeapObject::LArray(elems) => {
                write!(f, "[")?;
                for (i, v) in elems.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", v)?;
                }
                write!(f, "]")
            }
            HeapObject::LStringMut(v) => {
                if let Ok(borrowed) = v.try_borrow() {
                    write!(f, "@\"{}\"", String::from_utf8_lossy(&borrowed))
                } else {
                    write!(f, "@\"<borrowed>\"")
                }
            }
            HeapObject::LBytes(b) => {
                write!(f, "#bytes[")?;
                for (i, byte) in b.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:02x}", byte)?;
                }
                write!(f, "]")
            }
            HeapObject::LBytesMut(b) => {
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
            HeapObject::External(ext) => write!(f, "#<{}>", ext.type_name),
            HeapObject::Parameter { id, .. } => write!(f, "<parameter:{}>", id),
            HeapObject::LSet(s) => write!(f, "LSet({:?})", s),
            HeapObject::LSetMut(s) => write!(f, "LSetMut({:?})", s.borrow()),
        }
    }
}

// Re-export arena types and functions so existing `use crate::value::heap::{...}`
// import sites continue working after the arena code moved to `arena.rs`.
pub use super::arena::{
    alloc, alloc_permanent, clone_heap, deref, drop_heap, heap_arena_capacity,
    heap_arena_checkpoint, heap_arena_len, heap_arena_mark, heap_arena_object_limit,
    heap_arena_peak, heap_arena_release, heap_arena_reset, heap_arena_reset_peak,
    heap_arena_set_object_limit, set_alloc_error, take_alloc_error, ArenaGuard, ArenaMark,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_string() {
        let v = alloc(HeapObject::LString("hello".into()));
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::LString(s) => assert_eq!(&**s, "hello"),
                _ => panic!("Expected LString"),
            }
        }
    }

    #[test]
    fn test_alloc_permanent_string() {
        let v = alloc_permanent(HeapObject::LString("permanent".into()));
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::LString(s) => assert_eq!(&**s, "permanent"),
                _ => panic!("Expected LString"),
            }
            drop_heap(v);
        }
    }

    #[test]
    fn test_arena_mark_release() {
        let mark = heap_arena_mark();
        let v = alloc(HeapObject::LString("temporary".into()));
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::LString(s) => assert_eq!(&**s, "temporary"),
                _ => panic!("Expected LString"),
            }
        }
        heap_arena_release(mark);
    }

    #[test]
    fn test_arena_guard_raii() {
        let before = heap_arena_len();
        {
            let _guard = ArenaGuard::new();
            alloc(HeapObject::LString("guarded".into()));
            alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
            let during = heap_arena_len();
            assert_eq!(during, before + 2);
        }
        let after = heap_arena_len();
        assert_eq!(after, before);
    }

    #[test]
    fn test_arena_nested_guards() {
        let before = heap_arena_len();
        {
            let _outer = ArenaGuard::new();
            alloc(HeapObject::LString("outer alloc".into()));
            {
                let _inner = ArenaGuard::new();
                alloc(HeapObject::LString("inner alloc".into()));
                let during_inner = heap_arena_len();
                assert_eq!(during_inner, before + 2);
            }
            let after_inner = heap_arena_len();
            assert_eq!(after_inner, before + 1);
        }
        let after_outer = heap_arena_len();
        assert_eq!(after_outer, before);
    }

    #[test]
    fn test_arena_guard_fires_on_error_path() {
        let before = heap_arena_len();
        let result: Result<(), String> = {
            let _guard = ArenaGuard::new();
            alloc(HeapObject::LString("will be freed".into()));
            alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
            Err("simulated error".to_string())
        };
        assert!(result.is_err());
        let after = heap_arena_len();
        assert_eq!(after, before);
    }

    #[test]
    fn test_heap_tag() {
        let s = HeapObject::LString("test".into());
        assert_eq!(s.tag(), HeapTag::LString);
        assert_eq!(s.type_name(), "string");
    }
}
