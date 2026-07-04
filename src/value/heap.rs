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
use crate::value::region_slice::RegionSlice;
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
    ClosureTemplate = 29,
}

/// All heap-allocated value types.
///
/// Each variant corresponds to a type that cannot be represented inline
/// in the tagged-union Value. Objects are allocated on the heap and accessed
/// via pointer.
///
/// 19 user-facing variants carry a `traits: Value` field (initialized to
/// `Value::NIL`). The infrastructure variants (Float, LibHandle, FFISignature,
/// FFIType, ClosureTemplate) do not carry traits. Native-fns are NOT here — they
/// are immediates (`Value{TAG_NATIVE_FN, prim_id}`), no heap cell.
pub enum HeapObject {
    /// Immutable string. Bytes stored inline in the arena.
    LString { s: RegionSlice<u8>, traits: Value },

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
        elements: RegionSlice<Value>,
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
        data: RegionSlice<u8>,
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
        data: RegionSlice<Value>,
        traits: Value,
    },

    /// Mutable set (BTreeSet wrapped in `Rc<RefCell>`) — Rc-shared for
    /// cross-fiber live-update semantics across `deep_copy_to_outbox`.
    LSetMut {
        data: std::rc::Rc<RefCell<BTreeSet<Value>>>,
        traits: Value,
    },

    /// A region-allocated closure **template** (code object). Materialized per
    /// execution by `MakeClosure` from a compile-time blueprint, into the same
    /// region as the closure instance that references it (docs/impl/region-model.md
    /// § "Constants lower as ordinary allocations" — closure templates are no
    /// exception). Reclaimed by region RC.
    /// Never user-visible: it carries no `traits` and is never compared,
    /// hashed, or serialized as a user value.
    ClosureTemplate(crate::value::closure::ClosureTemplate),
}

/// Thread handle for concurrent execution.
///
/// Holds the result of a spawned thread's execution plus a completion
/// channel the joiner waits on. `Arc<Mutex<>>` shares the result across
/// threads; the worker fills it before signalling completion.
///
/// Completion is delivered via the channel wake protocol (`done_rx` /
/// `done_wake`) rather than polling: after storing its result the worker
/// sends a sentinel on `done_rx`'s channel and signals `done_wake`, so a
/// joiner parked in `chan/select` wakes exactly once and yields to the
/// scheduler in the meantime. `sys/thread-state` peeks `result` first (a
/// finished thread needs no wait) and otherwise hands back a fresh
/// `chan/receiver` over `done_rx` for the caller to select on. These are
/// plain `Send` fields (like `result`) — not heap `Value`s — so they need
/// no GC tracing.
#[derive(Clone)]
pub struct ThreadHandle {
    /// The result of the spawned thread execution, wrapped in `SendBundle` for Send.
    pub result: Arc<Mutex<Option<Result<crate::value::send::SendBundle, String>>>>,
    /// Receiver half of the worker's completion channel. Cloned into a
    /// fresh `chan/receiver` Value on demand (`sys/thread-state`).
    /// `pub(crate)`: `SendableValue` is a crate-private type.
    pub(crate) done_rx: crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
    /// Shared wake list for the completion channel — the worker signals
    /// it so a parked `chan/select` over `done_rx` wakes.
    pub(crate) done_wake: Arc<crate::primitives::chan::WakeList>,
}

impl ThreadHandle {
    /// Create a new thread handle with a shared result slot and the
    /// receiver/wake-list halves of its completion channel.
    /// `pub(crate)`: takes the crate-private `SendableValue` channel type.
    pub(crate) fn new(
        result: Arc<Mutex<Option<Result<crate::value::send::SendBundle, String>>>>,
        done_rx: crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
        done_wake: Arc<crate::primitives::chan::WakeList>,
    ) -> Self {
        ThreadHandle {
            result,
            done_rx,
            done_wake,
        }
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

mod objimpl;

// Re-export arena types and functions so existing `use crate::value::heap::{...}`
// import sites continue working after the arena code moved to `arena.rs`.
pub use super::arena::{alloc, alloc_root, deref, drop_heap};

#[cfg(test)]
mod tests;
