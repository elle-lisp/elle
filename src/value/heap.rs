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
pub struct Cons {
    pub first: Value,
    pub rest: Value,
    pub traits: Value,
}

impl Cons {
    pub fn new(first: Value, rest: Value) -> Self {
        Cons {
            first,
            rest,
            traits: Value::NIL,
        }
    }
}

impl std::fmt::Debug for Cons {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?} . {:?})", self.first, self.rest)
    }
}

impl Clone for Cons {
    fn clone(&self) -> Self {
        Cons {
            first: self.first,
            rest: self.rest,
            traits: self.traits,
        }
    }
}

impl PartialEq for Cons {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first && self.rest == other.rest
        // traits intentionally excluded
    }
}

impl Eq for Cons {}

impl std::hash::Hash for Cons {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.first.hash(state);
        self.rest.hash(state);
        // traits intentionally excluded
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
        // traits intentionally excluded
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
    LBox = 8,
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
///
/// 19 user-facing variants carry a `traits: Value` field (initialized to
/// `Value::NIL`). The 6 infrastructure variants (Float, NativeFn, LibHandle,
/// Binding, FFISignature, FFIType) do not carry traits.
pub enum HeapObject {
    /// Immutable string
    LString { s: Box<str>, traits: Value },

    /// Cons cell (list pair)
    Cons(Cons),

    /// Mutable array
    LArrayMut {
        data: RefCell<Vec<Value>>,
        traits: Value,
    },

    /// Mutable struct (hash map)
    LStructMut {
        data: RefCell<BTreeMap<TableKey, Value>>,
        traits: Value,
    },

    /// Immutable struct
    LStruct {
        data: BTreeMap<TableKey, Value>,
        traits: Value,
    },

    /// Function closure (interpreted)
    Closure { closure: Rc<Closure>, traits: Value },

    /// Immutable array (fixed-length sequence)
    LArray { elements: Vec<Value>, traits: Value },

    /// Mutable @string (byte sequence)
    LStringMut {
        data: RefCell<Vec<u8>>,
        traits: Value,
    },

    /// Immutable byte sequence (binary data)
    LBytes { data: Vec<u8>, traits: Value },

    /// Mutable byte sequence (binary data workspace)
    LBytesMut {
        data: RefCell<Vec<u8>>,
        traits: Value,
    },

    /// Mutable box for captured variables.
    /// The boolean distinguishes compiler-created boxes (true, auto-unwrapped
    /// by LoadUpvalue) from user-created boxes via `box` (false, not auto-unwrapped).
    LBox {
        cell: RefCell<Value>,
        is_local: bool,
        traits: Value,
    },

    /// Float value that couldn't be stored inline (NaN payload)
    Float(f64),

    /// Native function (Rust function)
    NativeFn(NativeFn),

    /// FFI library handle
    LibHandle(u32),

    /// Thread handle for concurrent execution
    ThreadHandle {
        handle: crate::value::heap::ThreadHandle,
        traits: Value,
    },

    /// Fiber: independent execution context with its own stack and frames
    Fiber { handle: FiberHandle, traits: Value },

    /// Syntax object: preserves scope sets through the Value round-trip
    /// during macro expansion. This is the only HeapObject variant that
    /// references compile-time types — an intentional coupling required
    /// for first-class syntax objects in hygienic macros.
    Syntax { syntax: Rc<Syntax>, traits: Value },

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
    ManagedPointer {
        addr: std::cell::Cell<Option<usize>>,
        traits: Value,
    },

    /// Opaque external object from a plugin.
    /// Holds an arbitrary Rust value with a type name for Elle-side identity.
    External { obj: ExternalObject, traits: Value },

    /// Dynamic parameter (Racket-style). Each parameter has a unique id
    /// (for lookup in the fiber's param_frames stack) and a default value
    /// (returned when no parameterize binding is active).
    Parameter {
        id: u32,
        default: Value,
        traits: Value,
    },

    /// Immutable set (BTreeSet, no RefCell)
    LSet {
        data: BTreeSet<Value>,
        traits: Value,
    },

    /// Mutable set (BTreeSet wrapped in RefCell)
    LSetMut {
        data: RefCell<BTreeSet<Value>>,
        traits: Value,
    },
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
    /// The result of the spawned thread execution, wrapped in `SendBundle` for Send.
    pub result: Arc<Mutex<Option<Result<crate::value::send::SendBundle, String>>>>,
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

impl Clone for ExternalObject {
    fn clone(&self) -> Self {
        ExternalObject {
            type_name: self.type_name,
            data: self.data.clone(),
        }
    }
}

impl HeapObject {
    /// Get the type tag for this heap object.
    #[inline]
    pub fn tag(&self) -> HeapTag {
        match self {
            HeapObject::LString { .. } => HeapTag::LString,
            HeapObject::Cons(_) => HeapTag::Cons,
            HeapObject::LArrayMut { .. } => HeapTag::LArrayMut,
            HeapObject::LStructMut { .. } => HeapTag::LStructMut,
            HeapObject::LStruct { .. } => HeapTag::LStruct,
            HeapObject::Closure { .. } => HeapTag::Closure,
            HeapObject::LArray { .. } => HeapTag::LArray,
            HeapObject::LStringMut { .. } => HeapTag::LStringMut,
            HeapObject::LBytes { .. } => HeapTag::LBytes,
            HeapObject::LBytesMut { .. } => HeapTag::LBytesMut,
            HeapObject::LBox { .. } => HeapTag::LBox,
            HeapObject::Float(_) => HeapTag::Float,
            HeapObject::NativeFn(_) => HeapTag::NativeFn,
            HeapObject::LibHandle(_) => HeapTag::LibHandle,
            HeapObject::ThreadHandle { .. } => HeapTag::ThreadHandle,
            HeapObject::Fiber { .. } => HeapTag::Fiber,
            HeapObject::Syntax { .. } => HeapTag::Syntax,
            HeapObject::Binding(_) => HeapTag::Binding,
            HeapObject::FFISignature(_, _) => HeapTag::FFISignature,
            HeapObject::FFIType(_) => HeapTag::FFIType,
            HeapObject::ManagedPointer { .. } => HeapTag::ManagedPointer,
            HeapObject::External { .. } => HeapTag::External,
            HeapObject::Parameter { .. } => HeapTag::Parameter,
            HeapObject::LSet { .. } => HeapTag::LSet,
            HeapObject::LSetMut { .. } => HeapTag::LSetMut,
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            HeapObject::LString { .. } => "string",
            HeapObject::Cons(_) => "list",
            HeapObject::LArrayMut { .. } => "@array",
            HeapObject::LStructMut { .. } => "@struct",
            HeapObject::LStruct { .. } => "struct",
            HeapObject::Closure { .. } => "closure",
            HeapObject::LArray { .. } => "array",
            HeapObject::LStringMut { .. } => "@string",
            HeapObject::LBytes { .. } => "bytes",
            HeapObject::LBytesMut { .. } => "@bytes",
            HeapObject::LBox { .. } => "box",
            HeapObject::Float(_) => "float",
            HeapObject::NativeFn(_) => "native-function",
            HeapObject::LibHandle(_) => "library-handle",
            HeapObject::ThreadHandle { .. } => "thread-handle",
            HeapObject::Fiber { .. } => "fiber",
            HeapObject::Syntax { .. } => "syntax",
            HeapObject::Binding(_) => "binding",
            HeapObject::FFISignature(_, _) => "ffi-signature",
            HeapObject::FFIType(_) => "ffi-type",
            HeapObject::ManagedPointer { .. } => "pointer",
            HeapObject::External { obj, .. } => obj.type_name,
            HeapObject::Parameter { .. } => "parameter",
            HeapObject::LSet { .. } => "set",
            HeapObject::LSetMut { .. } => "@set",
        }
    }
}

impl std::fmt::Debug for HeapObject {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            HeapObject::LString { s, .. } => write!(f, "\"{}\"", s),
            HeapObject::Cons(c) => write!(f, "({:?} . {:?})", c.first, c.rest),
            HeapObject::LArrayMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    write!(f, "{:?}", &*borrowed)
                } else {
                    write!(f, "[<borrowed>]")
                }
            }
            HeapObject::LStructMut { .. } => write!(f, "<@struct>"),
            HeapObject::LStruct { .. } => write!(f, "<struct>"),
            HeapObject::Closure { .. } => write!(f, "<closure>"),
            HeapObject::LArray { elements, .. } => {
                write!(f, "[")?;
                for (i, v) in elements.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:?}", v)?;
                }
                write!(f, "]")
            }
            HeapObject::LStringMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    write!(f, "@\"{}\"", String::from_utf8_lossy(&borrowed))
                } else {
                    write!(f, "@\"<borrowed>\"")
                }
            }
            HeapObject::LBytes { data, .. } => {
                write!(f, "#bytes[")?;
                for (i, byte) in data.iter().enumerate() {
                    if i > 0 {
                        write!(f, " ")?;
                    }
                    write!(f, "{:02x}", byte)?;
                }
                write!(f, "]")
            }
            HeapObject::LBytesMut { data, .. } => {
                if let Ok(borrowed) = data.try_borrow() {
                    write!(f, "#@bytes[")?;
                    for (i, byte) in borrowed.iter().enumerate() {
                        if i > 0 {
                            write!(f, " ")?;
                        }
                        write!(f, "{:02x}", byte)?;
                    }
                    write!(f, "]")
                } else {
                    write!(f, "#@bytes[<borrowed>]")
                }
            }
            HeapObject::LBox { .. } => write!(f, "<box>"),
            HeapObject::Float(n) => write!(f, "{}", n),
            HeapObject::NativeFn(_) => write!(f, "<native-fn>"),
            HeapObject::LibHandle(id) => write!(f, "<lib-handle:{}>", id),
            HeapObject::ThreadHandle { .. } => write!(f, "<thread-handle>"),
            HeapObject::Fiber { handle, .. } => match handle.try_with(|fib| fib.status.as_str()) {
                Some(status) => write!(f, "<fiber:{}>", status),
                None => write!(f, "<fiber:taken>"),
            },
            HeapObject::Syntax { syntax, .. } => write!(f, "#<syntax:{}>", syntax),
            HeapObject::Binding(_) => write!(f, "#<binding>"),
            HeapObject::FFISignature(_, _) => write!(f, "<ffi-signature>"),
            HeapObject::FFIType(desc) => write!(f, "<ffi-type:{:?}>", desc),
            HeapObject::ManagedPointer { addr, .. } => match addr.get() {
                Some(a) => write!(f, "<managed-pointer 0x{:x}>", a),
                None => write!(f, "<freed-pointer>"),
            },
            HeapObject::External { obj, .. } => write!(f, "#<{}>", obj.type_name),
            HeapObject::Parameter { id, .. } => write!(f, "<parameter:{}>", id),
            HeapObject::LSet { data, .. } => write!(f, "LSet({:?})", data),
            HeapObject::LSetMut { data, .. } => write!(f, "LSetMut({:?})", data.borrow()),
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
        let v = alloc(HeapObject::LString {
            s: "hello".into(),
            traits: Value::NIL,
        });
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::LString { s, .. } => assert_eq!(&**s, "hello"),
                _ => panic!("Expected LString"),
            }
        }
    }

    #[test]
    fn test_alloc_permanent_string() {
        let v = alloc_permanent(HeapObject::LString {
            s: "permanent".into(),
            traits: Value::NIL,
        });
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::LString { s, .. } => assert_eq!(&**s, "permanent"),
                _ => panic!("Expected LString"),
            }
            drop_heap(v);
        }
    }

    #[test]
    fn test_arena_mark_release() {
        let mark = heap_arena_mark();
        let v = alloc(HeapObject::LString {
            s: "temporary".into(),
            traits: Value::NIL,
        });
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::LString { s, .. } => assert_eq!(&**s, "temporary"),
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
            alloc(HeapObject::LString {
                s: "guarded".into(),
                traits: Value::NIL,
            });
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
            alloc(HeapObject::LString {
                s: "outer alloc".into(),
                traits: Value::NIL,
            });
            {
                let _inner = ArenaGuard::new();
                alloc(HeapObject::LString {
                    s: "inner alloc".into(),
                    traits: Value::NIL,
                });
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
            alloc(HeapObject::LString {
                s: "will be freed".into(),
                traits: Value::NIL,
            });
            alloc(HeapObject::Cons(Cons::new(Value::NIL, Value::NIL)));
            Err("simulated error".to_string())
        };
        assert!(result.is_err());
        let after = heap_arena_len();
        assert_eq!(after, before);
    }

    #[test]
    fn test_heap_tag() {
        let s = HeapObject::LString {
            s: "test".into(),
            traits: Value::NIL,
        };
        assert_eq!(s.tag(), HeapTag::LString);
        assert_eq!(s.type_name(), "string");
    }
}
