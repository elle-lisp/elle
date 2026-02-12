//! CPS (Continuation-Passing Style) infrastructure for colorless coroutines
//!
//! This module provides the runtime machinery for coroutines:
//! - Continuations: reified stack frames that can be stored and resumed
//! - Actions: results from CPS code execution
//! - Trampoline: executor loop for CPS code
//! - Arena: efficient continuation allocation
//! - Primitives: yield and resume operations
//! - CPS transformation: selective CPS transformation for yielding expressions

mod action;
mod arena;
mod continuation;
mod cps_expr;
pub mod primitives;
mod trampoline;
mod transform;

pub use action::Action;
pub use arena::{ArenaStats, ContinuationArena};
pub use continuation::Continuation;
pub use cps_expr::CpsExpr;
pub use primitives::{coroutine_done, coroutine_status, coroutine_value, make_coroutine};
pub use trampoline::{Trampoline, TrampolineConfig, TrampolineResult};
pub use transform::CpsTransformer;
