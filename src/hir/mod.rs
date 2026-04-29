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

mod analyze;
pub mod arena;
pub mod binding;
pub mod dataflow;
mod defuse;
pub mod display;
mod expr;
pub mod functionalize;
pub mod lint;
mod liveness;
mod pattern;
pub mod region;
mod regions;
pub mod symbols;
pub mod tailcall;

pub use analyze::{AnalysisResult, Analyzer, FileForm};
pub use arena::{BindingArena, BindingInner, BindingScope};
pub use binding::{Binding, CaptureInfo, CaptureKind};
pub use dataflow::{analyze_dataflow, format_dataflow, DataflowInfo};
pub use defuse::ValueOrigin;
pub use expr::{
    reset_hir_ids, BlockId, CallArg, Hir, HirId, HirKind, IntrinsicOp, ParamBound, VarargKind,
};
pub use lint::HirLinter;
pub use liveness::BitSet;
pub use pattern::{HirPattern, PatternBindings, PatternKey, PatternLiteral};
pub use region::{CallClassification, Region, RegionInfo, RegionKind};
pub use regions::{analyze_regions, analyze_regions_with, format_regions};
pub use symbols::extract_symbols_from_hir;
