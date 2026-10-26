// audited: 2026-10-24
//! The signal solver: facts about a unit's HIR, linked through literal imports, and their least model.
//!
//! docs/impl/solver.md
//!
//! The analyzer solves each unit it analyzes with an empty set of module
//! shapes, so every import stays open. The spike in examples/signal_solve
//! links the files of a program through their shapes instead.

mod extract;
mod fixpoint;
mod link;
mod model;
mod writeback;

pub use extract::{AVal, CallSite, Extractor, ModuleShape, Pending, PendingExportCall, Unit};
pub use fixpoint::{bit_values, solve, Bits, Solved};
pub use link::{link, lower, Lowered};
pub use model::{
    all_bits, bit_positions, to_word, Facts, FileId, KeyId, Model, SiteId, Solution, VarId, VarKey,
    TOP,
};
pub use writeback::Answers;
