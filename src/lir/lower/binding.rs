//! Binding-related lowering: let, letrec, define, set
//!
//! Split by concern across submodules; all methods hang off the shared
//! `impl<'a> Lowerer<'a>`:
//!   - `let`:    scoped recursive forms (`lower_let`, `lower_letrec`)
//!   - `define`: mutating forms (`lower_define`, `lower_assign`)
//!   - `cell`:   cell/destructure delegations (`lower_make_cell`, …)
//!   - `destructure`: the recursive pattern-destructuring machinery.

use super::*;
use crate::hir::PatternKey;

mod cell;
mod define;
mod destructure;
mod r#let;
