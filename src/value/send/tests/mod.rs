// audited: 2026-10-06
//! What crosses the send boundary intact: closures and their LIR, ports and parameters, symbols, and the mirrors.
//!
//! docs/threads.md
//! docs/impl/symbol.md
//!
//! - `closures` — a closure's LIR and its abandoned-frame tables survive the trip.
//! - `ports` — a parameter and a stdio port cross by identity and by kind.
//! - `symbols` — a symbol or keyword names the same thing on both sides.
//! - `mirror` — the serde mirror keeps container kinds apart, and the syntax
//!   mirror rebuilds a tree in the destination's arena.

use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{InstrRef, LirConst, LirInstr, LirOwned, Reg, Terminator};
use crate::value::closure::{Closure, TemplateProto};
use crate::value::fiber::SignalBits;
use crate::value::heap::HeapObject;
use crate::value::types::Arity;
use std::rc::Rc;

/// Reconstruct a bundle/value through a ctx over a fresh region on a leaked test
/// heap, NOT releasing the region: the result must outlive the call (the test
/// reads it afterward), so freeing the region would recycle the pages the
/// returned value points at. The leaked heap keeps it resident for the test.
fn into_value_in_region(f: impl FnOnce(&mut crate::primitives::ctx::Alloc) -> Value) -> Value {
    let heap_ptr = crate::value::arena::leaked_test_heap();
    let region = unsafe { (*heap_ptr).new_runtime_region() };
    let mut ctx = crate::primitives::ctx::Alloc::with_region(region, unsafe { &mut *heap_ptr });
    f(&mut ctx)
}

mod closures;
mod mirror;
mod ports;
mod symbols;
