// audited: 2026-09-14
//! Region RC emission: the retain side, and what names a value for it.
//!
//! The release side is `regiondecref.rs`. Split by the question each submodule
//! answers, all methods hanging off the shared `impl<'a> Lowerer<'a>`:
//!   - `slots`:    which slot a region's value is read from, and its address space
//!   - `cells`:    what a 1-slot container's binder retains and stores
//!   - `coalesce`: whether a static slot may stand in for a value's region
//!   - `retain`:   the increfs and adopts a node's stores owe

use super::*;

mod cells;
mod coalesce;
mod retain;
mod slots;
