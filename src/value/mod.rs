//! Value representation and NaN-boxing architecture
//!
//! This module implements the core value type for the Elle VM using NaN-boxing,
//! a technique that encodes multiple types into a single 64-bit IEEE 754 double.

pub mod closure;
pub mod display;
pub mod error;
pub mod ffi;
pub mod fiber;
pub mod heap;
pub mod intern;
pub mod repr;
pub mod send;
pub mod types;

// Export the new NaN-boxed Value as the canonical Value type
pub use repr::{cons, list, Value};

// Export heap types
pub use heap::{Cons, HeapObject, HeapTag};

// Export arena management
pub use heap::{ArenaGuard, ArenaMark};

// Export error value construction
pub use error::{error_val, format_error};

// Export SendValue for thread-safe value transmission
pub use send::SendValue;

// Export core types
pub use types::{Arity, NativeFn, SymbolId, TableKey};

// Export closure and fiber types
pub use closure::Closure;
pub use fiber::{
    CallFrame, Fiber, FiberHandle, FiberStatus, Frame, SignalBits, SuspendedFrame, WeakFiberHandle,
    SIG_CANCEL, SIG_DEBUG, SIG_ERROR, SIG_HALT, SIG_OK, SIG_PROPAGATE, SIG_QUERY, SIG_RESUME,
    SIG_YIELD,
};

// Export FFI types
pub use ffi::LibHandle;

// Export ThreadHandle from heap
pub use heap::ThreadHandle;
