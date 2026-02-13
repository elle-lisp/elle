//! CPS (Continuation-Passing Style) infrastructure for colorless coroutines
//!
//! This module provides the runtime machinery for coroutines:
//! - Continuations: reified stack frames that can be stored and resumed
//! - Actions: results from CPS code execution
//! - Trampoline: executor loop for CPS code
//! - Arena: efficient continuation allocation
//! - Primitives: yield and resume operations
//! - CPS transformation: selective CPS transformation for yielding expressions
//! - JIT compilation: native code generation for CPS expressions
//! - Mixed calls: native code calling CPS functions
//! - Continuation pool: efficient continuation allocation
//! - Interpreter: tree-walking interpreter for CPS expressions

mod action;
mod arena;
mod cont_pool;
mod continuation;
mod cps_expr;
mod interpreter;
pub mod jit;
mod jit_action;
mod mixed_calls;
pub mod primitives;
mod trampoline;
mod transform;

pub use action::Action;
pub use arena::{ArenaStats, ContinuationArena};
pub use cont_pool::{
    clear_pool, get_done_continuation, pool_stats, return_continuation, ContinuationPool,
};
pub use continuation::Continuation;
pub use cps_expr::CpsExpr;
pub use interpreter::CpsInterpreter;
pub use jit::CpsJitCompiler;
pub use jit_action::{ActionTag, JitAction};
pub use mixed_calls::{jit_call_cps_function, jit_is_suspended_coroutine, jit_resume_coroutine};
pub use primitives::{coroutine_done, coroutine_status, coroutine_value, make_coroutine};
pub use trampoline::{Trampoline, TrampolineConfig, TrampolineResult};
pub use transform::CpsTransformer;
