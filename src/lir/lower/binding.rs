// audited: 2026-11-21
//! Lowering the binding forms: `let`, `letrec`, `def`, `var`, `assign`, and the
//! destructure any of them may bind through.
//!
//! src/lir/lower/overview.md
//! docs/bindings.md
//!
//! Split by concern across submodules; all methods hang off the shared
//! `impl<'a> Lowerer<'a>`:
//!   - `let`:    the scoped forms (`lower_let`, `lower_letrec`)
//!   - `define`: the mutating forms (`lower_define`, `lower_assign`)
//!   - `cell`:   the `Destructure` entry, the cell delegations, `lower_bind_value`
//!   - `destructure`: the recursive pattern-destructuring machinery.

use super::*;
use crate::hir::PatternKey;

mod cell;
mod define;
mod destructure;
mod r#let;

use destructure::RestBuilds;
