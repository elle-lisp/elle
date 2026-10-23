// audited: 2026-10-22
//! Heap-allocated value types for the tagged-union value system.
//!
//! docs/impl/values.md
//!
//! All non-immediate values (strings, cons cells, arrays, closures, and the
//! rest) are stored on the heap and accessed through `HeapObject`.

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
/// When the `ffi` feature is enabled, this holds a lazily-prepared libffi CIF,
/// boxed because an inline one would set the size of every heap object. When
/// disabled, it is a zero-cost unit type — FFI signatures can still be created
/// and stored, but `ffi/call` (which needs the CIF) is unavailable.
#[cfg(feature = "ffi")]
pub type CifCache = RefCell<Option<Box<libffi::middle::Cif>>>;
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
        Pair::with_traits(first, rest, Value::NIL)
    }

    /// A pair carrying a trait table. Every other traitable variant takes its
    /// table as a field of the `HeapObject` arm, so a caller that rebuilds one
    /// — the image dumper's compacting copy — needs the same reach here.
    pub fn with_traits(first: Value, rest: Value, traits: Value) -> Self {
        Pair {
            first,
            rest,
            traits,
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

/// Where a capture cell was minted — the fact the image dumper's snap
/// decision reads (docs/impl/image/sealing.md).
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum CellOrigin {
    /// Minted at run time (`populate_env`, the JIT prologue, `send`): no
    /// binding identity, so the dumper never snaps it.
    Runtime,
    /// A compiled forward cell for the binding named `name`; `mutated` is
    /// whether the compilation unit assigns that binding anywhere.
    Compiled {
        name: crate::value::SymbolId,
        mutated: bool,
    },
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
/// Every variant but the five infrastructure ones (Float, LibHandle,
/// FFISignature, FFIType, ClosureTemplate) carries a `traits: Value` field,
/// initialized to `Value::NIL`. Native-fns are NOT here — they
/// are immediates (`Value{TAG_NATIVE_FN, prim_id}`), no heap cell.
///
/// `repr(u64)`, and each discriminant is the variant's `HeapTag`, so an
/// object's first word is its tag in every build (docs/impl/values.md).
#[repr(u64)]
pub enum HeapObject {
    /// Immutable string. Bytes stored inline in the arena.
    LString { s: RegionSlice<u8>, traits: Value } = HeapTag::LString as u64,

    /// Pair cell (list pair)
    Pair(Pair) = HeapTag::Pair as u64,

    /// Mutable array.
    ///
    /// The store is a growable `Vec` on the Rust heap behind a `RefCell`,
    /// not a region-inline `RegionSlice` like its immutable twin `LArray`:
    /// `push` grows it, and a region slice is fixed-length once allocated.
    /// The `RefCell` is the mutable-store seam — every write goes through a
    /// tracked funnel (`push_with_incref`, …) so the region's outgoing edges
    /// stay recorded (docs/impl/region/rules.md, src/value/AGENTS.md).
    LArrayMut {
        data: std::rc::Rc<RefCell<Vec<Value>>>,
        traits: Value,
    } = HeapTag::LArrayMut as u64,

    /// Mutable struct (hash map). See `LArrayMut` for the growable-store
    /// rationale.
    LStructMut {
        data: std::rc::Rc<RefCell<BTreeMap<TableKey, Value>>>,
        traits: Value,
    } = HeapTag::LStructMut as u64,

    /// Immutable struct: entries sorted by key, inline in the arena. A
    /// `TableKey` owns no Rust-heap allocation and a stored key's payload is
    /// interned into this struct's own region, so the whole entry slice is
    /// page bytes (docs/impl/values.md).
    LStruct {
        data: RegionSlice<(TableKey, Value)>,
        traits: Value,
    } = HeapTag::LStruct as u64,

    /// Function closure (interpreted). The `Closure` lives by value in the
    /// arena alongside its `HeapObject` header. Its code object is a separate
    /// region allocation it names by `Value`, so cloning a `Closure` copies
    /// three `Copy` fields.
    Closure { closure: Closure, traits: Value } = HeapTag::Closure as u64,

    /// Immutable array (fixed-length sequence, inline in arena)
    LArray {
        elements: RegionSlice<Value>,
        traits: Value,
    } = HeapTag::LArray as u64,

    /// Mutable @string (byte sequence). See `LArrayMut` for the
    /// growable-store rationale.
    LStringMut {
        data: std::rc::Rc<RefCell<Vec<u8>>>,
        traits: Value,
    } = HeapTag::LStringMut as u64,

    /// Immutable byte sequence (binary data, inline in arena)
    LBytes {
        data: RegionSlice<u8>,
        traits: Value,
    } = HeapTag::LBytes as u64,

    /// Mutable byte sequence (binary data workspace). See `LArrayMut` for
    /// the growable-store rationale.
    LBytesMut {
        data: std::rc::Rc<RefCell<Vec<u8>>>,
        traits: Value,
    } = HeapTag::LBytesMut as u64,

    /// User-facing mutable box, created via `(box v)`.
    /// Not auto-unwrapped by LoadUpvalue. The `RefCell` is the mutable-store
    /// seam — see `LArrayMut`.
    LBox {
        cell: std::rc::Rc<RefCell<Value>>,
        traits: Value,
    } = HeapTag::LBox as u64,

    /// Compiler-created capture cell for mutable captured variables.
    /// Auto-unwrapped by LoadUpvalue; never visible to user code.
    /// Every closure capturing the variable holds this one slot, so a write
    /// through any of them is visible to all — that is what makes a captured
    /// mutable binding shared rather than copied. Stores go through
    /// `capture_store_with_rebind` (see `LArrayMut`).
    CaptureCell {
        cell: std::rc::Rc<RefCell<Value>>,
        origin: CellOrigin,
        traits: Value,
    } = HeapTag::CaptureCell as u64,

    /// Float value that couldn't be stored inline (NaN payload)
    Float(f64) = HeapTag::Float as u64,

    /// FFI library handle
    LibHandle(u32) = HeapTag::LibHandle as u64,

    /// Thread handle for concurrent execution
    ThreadHandle {
        handle: crate::value::heap::ThreadHandle,
        traits: Value,
    } = HeapTag::ThreadHandle as u64,

    /// Fiber: independent execution context with its own stack and frames
    Fiber { handle: FiberHandle, traits: Value } = HeapTag::Fiber as u64,

    /// Syntax object: preserves scope sets through the Value round-trip
    /// during macro expansion. This is the only HeapObject variant that
    /// references compile-time types — an intentional coupling required
    /// for first-class syntax objects in hygienic macros.
    ///
    /// The node is stored inline, not behind a `Box`: `Syntax` is region-
    /// resident POD, and `value::build::syntax` copies the whole tree into
    /// this object's own region. The object is therefore self-contained —
    /// page bytes ARE the value, with no Rust-heap ownership inside and no
    /// cross-region edge to record (docs/impl/syntax.md).
    Syntax { syntax: Syntax, traits: Value } = HeapTag::Syntax as u64,

    /// Reified FFI function signature with optional cached CIF.
    /// The CIF is lazily prepared on first use and reused thereafter.
    /// When the `ffi` feature is disabled, the CIF cache is a unit type.
    FFISignature(crate::ffi::types::Signature, CifCache) = HeapTag::FFISignature as u64,

    /// Reified FFI compound type descriptor (struct or array layout)
    FFIType(crate::ffi::types::TypeDesc) = HeapTag::FFIType as u64,

    /// Managed FFI pointer with lifecycle tracking.
    /// `Some(addr)` = live, `None` = freed. Only for ffi/malloc'd memory.
    ManagedPointer {
        addr: std::cell::Cell<Option<usize>>,
        traits: Value,
    } = HeapTag::ManagedPointer as u64,

    /// External object from a plugin or the runtime: an arbitrary Rust value
    /// with a type name for Elle-side identity. The region scan cannot see
    /// into its payload.
    External { obj: ExternalObject, traits: Value } = HeapTag::External as u64,

    /// Dynamic parameter (Racket-style). Each parameter has a unique id
    /// (for lookup in the fiber's param_frames stack) and a default value
    /// (returned when no parameterize binding is active).
    Parameter {
        id: u32,
        default: Value,
        traits: Value,
    } = HeapTag::Parameter as u64,

    /// Immutable set (sorted array of values, inline in arena)
    LSet {
        data: RegionSlice<Value>,
        traits: Value,
    } = HeapTag::LSet as u64,

    /// Mutable set (BTreeSet wrapped in `Rc<RefCell>`). See `LArrayMut` for
    /// the growable-store rationale.
    LSetMut {
        data: std::rc::Rc<RefCell<BTreeSet<Value>>>,
        traits: Value,
    } = HeapTag::LSetMut as u64,

    /// A region-allocated code object **header**: one slice naming its
    /// payload. `MakeClosure` allocates one per closure creation, into the same
    /// region as the closure instance that references it, as every constant
    /// lowers (docs/impl/region/model.md). Reclaimed by region RC. Its
    /// payload — bytecode, constants, locations, region tables, LIR — is
    /// shared by every header over it and lives in its compile unit's code
    /// region (docs/impl/region/template.md).
    /// Never user-visible: it carries no `traits` and is never compared,
    /// hashed, or serialized as a user value.
    ClosureTemplate(crate::value::closure::ClosureTemplate) = HeapTag::ClosureTemplate as u64,
}

/// How a spawned thread ended, as its handle reports it (docs/threads.md).
pub enum WorkerOutcome {
    /// The closure answered a value, serialized for the joiner's heap.
    Ready(crate::value::send::SendBundle),
    /// The closure raised, or the thread panicked: the message the join
    /// raises as `:thread-error`.
    Failed(String),
    /// The kill switch was pulled before the worker ended, and the join
    /// raises `:killed`.
    Killed,
}

/// The slot a worker fills once, and its handle reads: empty while the
/// thread runs.
pub type OutcomeSlot = Arc<Mutex<Option<WorkerOutcome>>>;

/// Thread handle for concurrent execution.
///
/// Holds the outcome of a spawned thread's execution, the kill switch that
/// ends it, and a completion channel the joiner waits on. `Arc<Mutex<>>`
/// shares the outcome across threads; the worker fills it before signalling
/// completion.
///
/// Completion is delivered via the channel wake protocol (`done_rx` /
/// `done_wake`) rather than polling: after storing its outcome the worker
/// sends a sentinel on `done_rx`'s channel and signals `done_wake`, so a
/// joiner parked in `chan/select` wakes exactly once and yields to the
/// scheduler in the meantime. `sys/thread-state` peeks `result` first (a
/// finished thread needs no wait) and otherwise hands back a fresh
/// `chan/receiver` over `done_rx` for the caller to select on. These are
/// plain `Send` fields (like `result`) — not heap `Value`s — so they carry
/// no region edge.
#[derive(Clone)]
pub struct ThreadHandle {
    /// The outcome of the spawned thread's execution.
    pub result: OutcomeSlot,
    /// Receiver half of the worker's completion channel. Cloned into a
    /// fresh `chan/receiver` Value on demand (`sys/thread-state`).
    /// `pub(crate)`: `SendableValue` is a crate-private type.
    pub(crate) done_rx: crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
    /// Shared wake list for the completion channel — the worker signals
    /// it so a parked `chan/select` over `done_rx` wakes.
    pub(crate) done_wake: Arc<crate::primitives::chan::WakeList>,
    /// The spawner's end of the worker's kill switch, which `sys/kill` pulls
    /// (docs/threads.md).
    pub(crate) kill: crate::value::fiber::KillSwitch,
}

impl ThreadHandle {
    /// Create a new thread handle with a shared outcome slot, the
    /// receiver/wake-list halves of its completion channel, and the
    /// spawner's end of the worker's kill switch.
    /// `pub(crate)`: takes the crate-private `SendableValue` channel type.
    pub(crate) fn new(
        result: OutcomeSlot,
        done_rx: crossbeam_channel::Receiver<crate::primitives::chan::SendableValue>,
        done_wake: Arc<crate::primitives::chan::WakeList>,
        kill: crate::value::fiber::KillSwitch,
    ) -> Self {
        ThreadHandle {
            result,
            done_rx,
            done_wake,
            kill,
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

/// External object for a plugin-provided or runtime type.
/// Holds a type name (for Elle-side identity) and an arbitrary Rust value.
pub struct ExternalObject {
    pub type_name: &'static str,
    pub data: Rc<dyn Any>,
}

impl ExternalObject {
    /// An external over `data`. The region scan cannot see into its payload.
    pub fn opaque(type_name: &'static str, data: Rc<dyn Any>) -> Self {
        ExternalObject { type_name, data }
    }
}

impl Clone for ExternalObject {
    fn clone(&self) -> Self {
        ExternalObject::opaque(self.type_name, self.data.clone())
    }
}

mod objimpl;

// The arena functions, re-exported where a heap-object caller looks for them.
pub use super::arena::{alloc, alloc_root, deref, drop_heap};

#[cfg(test)]
mod tests;
