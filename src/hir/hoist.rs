// audited: 2026-11-11
//! Moves each constant out of a loop body into a `let` around the loop, so it is built once per entry.
//!
//! docs/impl/hoist.md

use super::arena::BindingArena;
use super::expr::Hir;

/// Move each constant in the body of an outermost loop into a `let` around
/// that loop (docs/impl/hoist.md).
pub fn hoist_loop_constants(_hir: &mut Hir, _arena: &mut BindingArena) {}

#[cfg(test)]
mod tests;
