//! Value representation and NaN-boxing architecture
//!
//! This module implements the core value type for the Elle VM using NaN-boxing,
//! a technique that encodes multiple types into a single 64-bit IEEE 754 double.

pub mod closure;
pub mod condition;
pub mod continuation;
pub mod coroutine;
pub mod display;
pub mod ffi;
pub mod heap;
pub mod intern;
pub mod repr;
pub mod send;
pub mod types;

// Export the new NaN-boxed Value as the canonical Value type
pub use repr::{cons, list, Value};

// Export heap types
pub use heap::{Cons, HeapObject, HeapTag};

// Export Condition for exception handling
pub use condition::Condition;

// Export SendValue for thread-safe value transmission
pub use send::SendValue;

// Export continuation types
pub use continuation::{ContinuationData, ContinuationFrame, ExceptionHandler};

// Export core types
pub use types::{Arity, NativeFn, SymbolId, TableKey, VmAwareFn};

// Export closure and coroutine types
pub use closure::Closure;
pub use coroutine::{Coroutine, CoroutineState};

// Export FFI types
pub use ffi::{CHandle, LibHandle, ThreadHandle};
