//! Value representation and tagged-union architecture
//!
//! This module implements the core value type for the Elle VM using a 16-byte
//! tagged union (tag: u64, payload: u64).

pub mod allocator;
pub mod arena;
pub mod closure;
pub mod cycle;
pub mod display;
pub mod error;
pub mod ffi;
pub mod fiber;
pub mod fiberheap;
pub mod heap;
pub mod inline_slice;
pub mod intern;
pub mod keyword;
pub mod repr;
pub mod send;
pub mod shared_alloc;
pub mod types;

// Export the tagged-union Value as the canonical Value type
pub use repr::{list, pair, Value};

// Export heap types
pub use heap::{HeapObject, HeapTag, Pair};

// Export arena management
pub use heap::{ArenaGuard, ArenaMark};

// Export error value construction
pub use error::{error_val, error_val_extra, format_error};

// Export SendValue and SendBundle for thread-safe value transmission
pub use send::SendBundle;
pub use send::SendValue;

// Export core types
pub use types::{
    sorted_struct_contains, sorted_struct_get, sorted_struct_insert, sorted_struct_remove, Arity,
    NativeFn, SymbolId, TableKey,
};

// Export fiber heap
pub use fiberheap::FiberHeap;

// Export closure and fiber types
pub use closure::{Closure, ClosureTemplate};
pub use fiber::{
    BytecodeFrame, CallFrame, Fiber, FiberHandle, FiberStatus, Frame, SignalBits, SuspendedFrame,
    WeakFiberHandle, SIG_ABORT, SIG_DEBUG, SIG_ERROR, SIG_FUEL, SIG_HALT, SIG_IO, SIG_OK,
    SIG_PROPAGATE, SIG_QUERY, SIG_RESUME, SIG_SWITCH, SIG_TERMINAL, SIG_YIELD,
};

// Export custom allocator types
pub use allocator::{AllocatorBox, ElleAllocator};

// Export FFI types
pub use ffi::LibHandle;

// Export ThreadHandle from heap
pub use heap::ThreadHandle;
