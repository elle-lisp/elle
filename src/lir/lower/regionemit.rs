// audited: 2026-11-10
//! Region RC emission from the solver's `RegionInfo`.
//!
//! The release side is `regiondecref.rs`. Split by the question each submodule
//! answers, all methods hanging off the shared `impl<'a> Lowerer<'a>`:
//!   - `slots`:    which slot a region's value is read from, and its address space
//!   - `cells`:    what a 1-slot container's binder retains for its init
//!   - `cellstore`: how a captured binding's binder stores its init, and drops it
//!   - `coalesce`: whether a static slot may stand in for a value's region
//!   - `retain`:   the increfs and adopts a node's stores owe

use super::*;

mod cells;
mod cellstore;
mod coalesce;
mod retain;
mod slots;
