//! Fiber execution: resume, propagate, abort, cancel.
//!
//! All fiber operations follow the same swap protocol:
//! 1. Take child fiber out of its handle
//! 2. Wire parent/child chain (Janet semantics)
//! 3. Swap parent out, child in
//! 4. Execute the child
//! 5. Set provisional status (Dead or Suspended)
//! 6. Extract result
//! 7. Swap back
//! 8. Put child back into its handle
//!
//! Status finalization happens in the caller, not in `with_child_fiber`:
//! - Resume: SIG_ERROR + uncaught by mask → Error (terminal)
//! - Resume: SIG_ERROR + caught by mask → Suspended (resumable)
//! - Abort: inject error + resume, result handled like resume (no stomp)
//! - Cancel: hard kill — set status to Error, drop frames, no resume
//!
//! SIG_TERMINAL signals are uncatchable — they pass through mask checks.

#[cfg(feature = "jit")]
use crate::jit::JitValue;
use crate::value::fiber::FiberStatus;
use crate::value::{
    BytecodeFrame, FiberHandle, SignalBits, SuspendedFrame, Value, SIG_ERROR, SIG_FUEL, SIG_HALT,
    SIG_OK, SIG_SWITCH, SIG_YIELD,
};
use std::rc::Rc;

use super::core::VM;

mod abort;
mod catch;
mod child;
mod jit;
mod owned;
mod param;
mod propagate;
mod refcount;
mod resume;
mod signal;
mod trampoline;

#[cfg(all(test, debug_assertions))]
mod borrow_tests;

// Re-export the moved free items so every path that previously resolved as
// `crate::vm::fiber::<Item>` (external callers) or `super::<Item>` (sibling
// submodules) still resolves unchanged. Visibility matches each item's original
// `pub(crate)`/private declaration.
pub(crate) use owned::{kill_fiber, parked_owner_nodes, release_fiber_owned, take_fiber_owned};
use refcount::{incref_signal_region, release_discarded_signal};
pub(crate) use refcount::{
    is_terminal_signal, release_displaced_denial_payload, release_displaced_terminal_signal,
    release_parked_signal,
};
// Re-exported for the WASM resume path (`crate::vm::fiber::record_terminal_signal_park`),
// the only external caller; `owned::kill_fiber` reaches it by module path. Unused
// re-export in a build without the WASM tier.
#[cfg_attr(not(feature = "wasm"), allow(unused_imports))]
pub(crate) use refcount::record_terminal_signal_park;

#[cfg(debug_assertions)]
pub(crate) use param::{first_stale_borrow, record_param_borrows};
pub(crate) use param::{flatten_param_frames, retain_param_baseline};
