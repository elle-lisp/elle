//! Heap-allocated value types for the tagged-union value system.
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
use crate::value::inline_slice::InlineSlice;
use crate::value::Value;

// Re-export types for convenience
pub use crate::value::closure::Closure;
pub use crate::value::types::{Arity, NativeFn, PrimFn, TableKey};

/// CIF cache type for FFI signatures.
///
/// When the `ffi` feature is enabled, this holds a lazily-prepared libffi CIF.
/// When disabled, it is a zero-cost unit type — FFI signatures can still be
/// created and stored, but `ffi/call` (which needs the CIF) is unavailable.
#[cfg(feature = "ffi")]
pub type CifCache = RefCell<Option<libffi::middle::Cif>>;
#[cfg(not(feature = "ffi"))]
pub type CifCache = ();

/// Pair cell for list construction.
pub struct Pair {
    pub first: Value,
    pub rest: Value,
    pub traits: Value,
}

impl Pair {
    pub fn new(first: Value, rest: Value) -> Self {
        Pair {
            first,
            rest,
            traits: Value::NIL,
        }
    }
}

impl std::fmt::Debug for Pair {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "({:?} . {:?})", self.first, self.rest)
    }
}

impl Clone for Pair {
    fn clone(&self) -> Self {
        Pair {
            first: self.first,
            rest: self.rest,
            traits: self.traits,
        }
    }
}

impl PartialEq for Pair {
    fn eq(&self, other: &Self) -> bool {
        self.first == other.first && self.rest == other.rest
        // traits intentionally excluded
    }
}

impl Eq for Pair {}

impl std::hash::Hash for Pair {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        self.first.hash(state);
        self.rest.hash(state);
        // traits intentionally excluded
    }
}

impl PartialOrd for Pair {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Pair {
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
    Pair = 1,
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
    CaptureCell = 28,
}

/// All heap-allocated value types.
///
/// Each variant corresponds to a type that cannot be represented inline
/// in the tagged-union Value. Objects are allocated on the heap and accessed
/// via pointer.
///
/// 19 user-facing variants carry a `traits: Value` field (initialized to
/// `Value::NIL`). The 5 infrastructure variants (Float, NativeFn, LibHandle,
/// FFISignature, FFIType) do not carry traits.
pub enum HeapObject {
    /// Immutable string. Bytes stored inline in the arena.
    LString { s: InlineSlice<u8>, traits: Value },

    /// Pair cell (list pair)
    Pair(Pair),

    /// Mutable array.
    ///
    /// `data` is `Rc<RefCell<...>>` so that cross-fiber sharing survives
    /// `deep_copy_to_outbox`: when a fiber yields an `@[]` through a
    /// request (e.g. `{:op :select :fibers pool}`), the outbox copy
    /// shares the same backing `Vec<Value>` as the original. Mutations
    /// made by one side are visible to the other — without this, the
    /// scheduler would see a snapshot of the pool at yield time and
    /// miss fibers pushed after it parked in `select-sets`.
    LArrayMut {
        data: std::rc::Rc<RefCell<Vec<Value>>>,
        traits: Value,
    },

    /// Mutable struct (hash map). See `LArrayMut` for the Rc-sharing
    /// rationale (cross-fiber live updates through yield).
    LStructMut {
        data: std::rc::Rc<RefCell<BTreeMap<TableKey, Value>>>,
        traits: Value,
    },

    /// Immutable struct (sorted array of key-value pairs).
    /// Keys may contain owned String data, so this stays on the Rust heap
    /// (Vec) rather than inline in the arena.
    LStruct {
        data: Vec<(TableKey, Value)>,
        traits: Value,
    },

    /// Function closure (interpreted). The `Closure` lives by value in the
    /// arena alongside its `HeapObject` header. `ClosureTemplate` remains
    /// `Rc`-shared across closure instances (bytecode, constants, location
    /// map, etc.), so cloning a `Closure` is O(1) (Rc bump + Copy fields).
    Closure { closure: Closure, traits: Value },

    /// Immutable array (fixed-length sequence, inline in arena)
    LArray {
        elements: InlineSlice<Value>,
        traits: Value,
    },

    /// Mutable @string (byte sequence). Rc-shared for cross-fiber
    /// live-update semantics across `deep_copy_to_outbox`.
    LStringMut {
        data: std::rc::Rc<RefCell<Vec<u8>>>,
        traits: Value,
    },

    /// Immutable byte sequence (binary data, inline in arena)
    LBytes {
        data: InlineSlice<u8>,
        traits: Value,
    },

    /// Mutable byte sequence (binary data workspace). Rc-shared for
    /// cross-fiber live-update semantics across `deep_copy_to_outbox`.
    LBytesMut {
        data: std::rc::Rc<RefCell<Vec<u8>>>,
        traits: Value,
    },

    /// User-facing mutable box, created via `(box v)`.
    /// Not auto-unwrapped by LoadUpvalue. Rc-shared for cross-fiber
    /// live-update semantics across `deep_copy_to_outbox`.
    LBox {
        cell: std::rc::Rc<RefCell<Value>>,
        traits: Value,
    },

    /// Compiler-created capture cell for mutable captured variables.
    /// Auto-unwrapped by LoadUpvalue; never visible to user code.
    /// Rc-shared so a mutation in a child fiber is visible to the parent
    /// when the closure crosses a yield boundary.
    CaptureCell {
        cell: std::rc::Rc<RefCell<Value>>,
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
    ///
    /// Uses `Box<Syntax>` rather than `Rc<Syntax>` because the tree is
    /// always cloned on extraction — `Rc` would add indirection without
    /// sharing benefits, and creates a dangling-pointer hazard when the
    /// slab slot is recycled.
    Syntax { syntax: Box<Syntax>, traits: Value },

    /// Reified FFI function signature with optional cached CIF.
    /// The CIF is lazily prepared on first use and reused thereafter.
    /// When the `ffi` feature is disabled, the CIF cache is a unit type.
    FFISignature(crate::ffi::types::Signature, CifCache),

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

    /// Immutable set (sorted array of values, inline in arena)
    LSet {
        data: InlineSlice<Value>,
        traits: Value,
    },

    /// Mutable set (BTreeSet wrapped in `Rc<RefCell>`) — Rc-shared for
    /// cross-fiber live-update semantics across `deep_copy_to_outbox`.
    LSetMut {
        data: std::rc::Rc<RefCell<BTreeSet<Value>>>,
        traits: Value,
    },
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
    /// Create a new thread handle with a shared result slot.
    pub fn new(result: Arc<Mutex<Option<Result<crate::value::send::SendBundle, String>>>>) -> Self {
        ThreadHandle { result }
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
            HeapObject::Pair(_) => HeapTag::Pair,
            HeapObject::LArrayMut { .. } => HeapTag::LArrayMut,
            HeapObject::LStructMut { .. } => HeapTag::LStructMut,
            HeapObject::LStruct { .. } => HeapTag::LStruct,
            HeapObject::Closure { .. } => HeapTag::Closure,
            HeapObject::LArray { .. } => HeapTag::LArray,
            HeapObject::LStringMut { .. } => HeapTag::LStringMut,
            HeapObject::LBytes { .. } => HeapTag::LBytes,
            HeapObject::LBytesMut { .. } => HeapTag::LBytesMut,
            HeapObject::LBox { .. } => HeapTag::LBox,
            HeapObject::CaptureCell { .. } => HeapTag::CaptureCell,
            HeapObject::Float(_) => HeapTag::Float,
            HeapObject::NativeFn(_) => HeapTag::NativeFn,
            HeapObject::LibHandle(_) => HeapTag::LibHandle,
            HeapObject::ThreadHandle { .. } => HeapTag::ThreadHandle,
            HeapObject::Fiber { .. } => HeapTag::Fiber,
            HeapObject::Syntax { .. } => HeapTag::Syntax,
            HeapObject::FFISignature(_, _) => HeapTag::FFISignature,
            HeapObject::FFIType(_) => HeapTag::FFIType,
            HeapObject::ManagedPointer { .. } => HeapTag::ManagedPointer,
            HeapObject::External { .. } => HeapTag::External,
            HeapObject::Parameter { .. } => HeapTag::Parameter,
            HeapObject::LSet { .. } => HeapTag::LSet,
            HeapObject::LSetMut { .. } => HeapTag::LSetMut,
        }
    }

    /// Get the Value-level TAG_* constant for this heap object.
    /// Used by the allocator to stamp the tag into the returned Value.
    #[inline]
    pub fn value_tag(&self) -> u64 {
        use crate::value::repr::{
            TAG_ARRAY, TAG_ARRAY_MUT, TAG_BYTES, TAG_BYTES_MUT, TAG_CAPTURE_CELL, TAG_CLOSURE,
            TAG_CONS, TAG_EXTERNAL, TAG_FFI_SIG, TAG_FFI_TYPE, TAG_FIBER, TAG_LBOX, TAG_LIB_HANDLE,
            TAG_MANAGED_PTR, TAG_NATIVE_FN, TAG_PARAMETER, TAG_SET, TAG_SET_MUT, TAG_STRING,
            TAG_STRING_MUT, TAG_STRUCT, TAG_STRUCT_MUT, TAG_SYNTAX, TAG_THREAD,
        };
        match self {
            HeapObject::LString { .. } => TAG_STRING,
            HeapObject::LStringMut { .. } => TAG_STRING_MUT,
            HeapObject::LArray { .. } => TAG_ARRAY,
            HeapObject::LArrayMut { .. } => TAG_ARRAY_MUT,
            HeapObject::LStruct { .. } => TAG_STRUCT,
            HeapObject::LStructMut { .. } => TAG_STRUCT_MUT,
            HeapObject::Pair(_) => TAG_CONS,
            HeapObject::Closure { .. } => TAG_CLOSURE,
            HeapObject::LBytes { .. } => TAG_BYTES,
            HeapObject::LBytesMut { .. } => TAG_BYTES_MUT,
            HeapObject::LSet { .. } => TAG_SET,
            HeapObject::LSetMut { .. } => TAG_SET_MUT,
            HeapObject::LBox { .. } => TAG_LBOX,
            HeapObject::CaptureCell { .. } => TAG_CAPTURE_CELL,
            HeapObject::Fiber { .. } => TAG_FIBER,
            HeapObject::Syntax { .. } => TAG_SYNTAX,
            HeapObject::NativeFn(_) => TAG_NATIVE_FN,
            HeapObject::FFISignature(_, _) => TAG_FFI_SIG,
            HeapObject::FFIType(_) => TAG_FFI_TYPE,
            HeapObject::LibHandle(_) => TAG_LIB_HANDLE,
            HeapObject::ManagedPointer { .. } => TAG_MANAGED_PTR,
            HeapObject::External { .. } => TAG_EXTERNAL,
            HeapObject::Parameter { .. } => TAG_PARAMETER,
            HeapObject::ThreadHandle { .. } => TAG_THREAD,
            // Float: in the new representation ALL floats are immediate (TAG_FLOAT,
            // payload = f64::to_bits()). HeapObject::Float must never be allocated.
            HeapObject::Float(_) => {
                panic!("HeapObject::Float must not be allocated — floats are now immediate")
            }
        }
    }

    /// Get a human-readable type name.
    pub fn type_name(&self) -> &'static str {
        match self {
            HeapObject::LString { .. } => "string",
            HeapObject::Pair(_) => "list",
            HeapObject::LArrayMut { .. } => "@array",
            HeapObject::LStructMut { .. } => "@struct",
            HeapObject::LStruct { .. } => "struct",
            HeapObject::Closure { .. } => "closure",
            HeapObject::LArray { .. } => "array",
            HeapObject::LStringMut { .. } => "@string",
            HeapObject::LBytes { .. } => "bytes",
            HeapObject::LBytesMut { .. } => "@bytes",
            HeapObject::LBox { .. } => "box",
            HeapObject::CaptureCell { .. } => "capture-cell",
            HeapObject::Float(_) => "float",
            HeapObject::NativeFn(_) => "native-fn",
            HeapObject::LibHandle(_) => "library-handle",
            HeapObject::ThreadHandle { .. } => "thread-handle",
            HeapObject::Fiber { .. } => "fiber",
            HeapObject::Syntax { .. } => "syntax",
            HeapObject::FFISignature(_, _) => "ffi-signature",
            HeapObject::FFIType(_) => "ffi-type",
            HeapObject::ManagedPointer { .. } => "ptr",
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
            HeapObject::LString { s, .. } => {
                write!(f, "\"{}\"", String::from_utf8_lossy(s.as_slice()))
            }
            HeapObject::Pair(c) => write!(f, "({:?} . {:?})", c.first, c.rest),
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
            HeapObject::CaptureCell { .. } => write!(f, "<capture-cell>"),
            HeapObject::Float(n) => write!(f, "{}", n),
            HeapObject::NativeFn(_) => write!(f, "<native-fn>"),
            HeapObject::LibHandle(id) => write!(f, "<lib-handle:{}>", id),
            HeapObject::ThreadHandle { .. } => write!(f, "<thread-handle>"),
            HeapObject::Fiber { handle, .. } => match handle.try_with(|fib| fib.status.as_str()) {
                Some(status) => write!(f, "<fiber:{}>", status),
                None => write!(f, "<fiber:taken>"),
            },
            HeapObject::Syntax { syntax, .. } => write!(f, "#<syntax:{}>", syntax),
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
    alloc, alloc_permanent, deref, drop_heap, heap_arena_len, heap_arena_mark, heap_arena_release,
    ArenaGuard, ArenaMark,
};

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_alloc_string() {
        let v = Value::string("hello");
        assert!(v.is_heap());
        assert_eq!(v.with_string(|s| s.to_string()).unwrap(), "hello");
    }

    #[test]
    fn test_alloc_permanent_cons() {
        // Pair has no inner arena allocation, safe for alloc_permanent.
        let v = alloc_permanent(HeapObject::Pair(Pair::new(Value::NIL, Value::int(1))));
        assert!(v.is_heap());
        unsafe {
            let obj = deref(v);
            match obj {
                HeapObject::Pair(c) => assert_eq!(c.rest.as_int(), Some(1)),
                _ => panic!("Expected Pair"),
            }
            drop_heap(v);
        }
    }

    #[test]
    fn test_arena_mark_release() {
        let mark = heap_arena_mark();
        let v = Value::string("temporary");
        assert!(v.is_heap());
        assert_eq!(v.with_string(|s| s.to_string()).unwrap(), "temporary");
        heap_arena_release(mark);
    }

    #[test]
    fn test_arena_guard_raii() {
        let before = heap_arena_len();
        {
            let _guard = ArenaGuard::new();
            Value::string("guarded");
            alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
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
            Value::string("outer alloc");
            {
                let _inner = ArenaGuard::new();
                Value::string("inner alloc");
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
            Value::string("will be freed");
            alloc(HeapObject::Pair(Pair::new(Value::NIL, Value::NIL)));
            Err("simulated error".to_string())
        };
        assert!(result.is_err());
        let after = heap_arena_len();
        assert_eq!(after, before);
    }

    #[test]
    fn test_heap_tag() {
        let v = Value::string("test");
        let s = unsafe { deref(v) };
        assert_eq!(s.tag(), HeapTag::LString);
        assert_eq!(s.type_name(), "string");
    }
}
