//! Value representation and tagged-union architecture
//!
//! This module implements the core value type for the Elle VM using a 16-byte
//! tagged union (tag: u64, payload: u64).

pub mod allocator;
pub mod arena;
pub mod build;
pub mod capturemask;
pub mod closure;
pub mod code;
pub mod cycle;
pub mod display;
pub mod error;
pub mod ffi;
pub mod fiber;
pub mod fiberheap;
pub mod heap;
#[cfg(test)]
pub mod intern;
pub mod keyword;
pub mod region_slice;
pub mod repr;
pub mod send;
pub mod template;
pub mod types;

// Export the tagged-union Value as the canonical Value type
pub use repr::Value;

// Export the compile-time heap-literal template
pub use template::ConstTemplate;

// Export heap types
pub use heap::{HeapObject, HeapTag, Pair};

// Export error value construction
pub use error::{error_val_extra_in, error_val_in, format_error, match_fail_error_in};

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

// Export the capture-slot bitmask seam
pub use capturemask::CaptureMask;

// Export closure and fiber types
pub use closure::{Closure, ClosureTemplate, TemplateProto, TemplateRef, VarargTag};
pub use code::Code;
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
