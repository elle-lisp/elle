//! High-level Intermediate Representation (HIR)
//!
//! HIR is the fully-analyzed form produced from expanded Syntax. All bindings
//! are resolved, signals are inferred, and captures are computed. This is the
//! input to the lowering phase that produces LIR.
//!
//! Pipeline:
//! ```text
//! Syntax → Expand → Syntax → Analyze → HIR → Lower → LIR → Emit → Bytecode
//! ```

pub(crate) mod analyze;
pub mod anf;
pub mod arena;
pub mod binding;
pub mod dataflow;
mod dead;
pub(crate) mod decision;
mod defuse;
pub mod display;
pub mod escape;
mod expr;
pub mod functionalize;
pub mod lint;
mod liveness;
mod narrow;
mod pattern;
pub mod region;
mod regularize;
mod return_incref;
pub mod symbols;
pub mod tailcall;
#[cfg(test)]
pub(crate) mod testkit;
pub mod typeinfer;
pub mod types;

pub use analyze::{classify_form, AnalysisResult, Analyzer, FileForm};
pub use arena::{BindingArena, BindingInner, BindingScope};
pub use binding::{Binding, CaptureInfo, CaptureKind};
pub use dataflow::{analyze_dataflow, format_dataflow, DataflowInfo};
pub use defuse::ValueOrigin;
pub use escape::{analyze_escape, EscapeInfo};
pub use expr::{
    reset_hir_ids, BlockId, CallArg, Hir, HirId, HirKind, IntrinsicOp, ParamBound, VarargKind,
};
pub use lint::HirLinter;
pub use liveness::BitSet;
pub use pattern::{HirPattern, PatternBindings, PatternKey, PatternLiteral};
pub(crate) use region::infer::return_frontier_regions;
pub use region::infer::{analyze_regions, analyze_regions_with, format_regions};
pub use region::{CallClassification, Region, RegionInfo};
pub(crate) use regularize::regularize;
pub use symbols::extract_symbols_from_hir;
