use super::*;
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

// Test bodies (and the ownership helpers in `support`) reference
// `super::ownership::…`. Once a `#[test]` moves into a sibling submodule,
// `super` names THIS module rather than the parent `crate::hir::regions`, so
// re-export the sibling `ownership` module here to keep `super::ownership`
// resolving from every submodule (the only `super::IDENT` reached from a test
// body is `super::ownership`).
use crate::hir::regions::ownership;

mod helpers;
mod support;
use helpers::*;
use support::*;

mod adopt;
mod basics;
mod blocks;
mod borrow;
mod cells;
mod compensate;
mod effects;
mod emit;
mod escape;
mod looprc;
mod merge;
mod owned;
mod realalloc;
mod reassign;
mod seeds;
