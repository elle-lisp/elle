//! CPS (Continuation-Passing Style) infrastructure for colorless coroutines
//!
//! This module provides the runtime machinery for coroutines:
//! - Continuations: reified stack frames that can be stored and resumed
//! - Actions: results from CPS code execution
//! - Trampoline: executor loop for CPS code
//! - Arena: efficient continuation allocation
//! - Primitives: yield and resume operations

mod action;
mod arena;
mod continuation;
pub mod primitives;
mod trampoline;

pub use action::Action;
pub use arena::{ArenaStats, ContinuationArena};
pub use continuation::Continuation;
pub use primitives::{coroutine_done, coroutine_status, coroutine_value, make_coroutine};
pub use trampoline::{Trampoline, TrampolineConfig, TrampolineResult};
