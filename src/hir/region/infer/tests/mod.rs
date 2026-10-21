// audited: 2026-10-21
//! The region-inference unit tests, one module per subject the solver decides.
//!
//! docs/impl/region/rules.md

use super::*;
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena};
use crate::primitives::register_primitives;
use crate::reader::read_syntax;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::vm::VM;

// Test bodies and the helpers in `support` name `super::ownership`. Inside a
// submodule `super` is this module, not `crate::hir::region::infer`, so this
// re-export keeps that path resolving.
use crate::hir::region::infer::ownership;

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
mod declared;
mod effects;
mod emit;
mod escape;
mod feeder;
mod inline;
mod letrec;
mod looprc;
mod merge;
mod owned;
mod patterns;
mod realalloc;
mod reassign;
mod restlist;
mod seeds;
