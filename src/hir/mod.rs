// audited: 2026-11-22
//! HIR: the tree the analyzer builds from expanded Syntax, and the passes that prepare it for lowering.
//!
//! docs/impl/hir.md

pub(crate) mod analyze;
pub mod anf;
pub mod arena;
pub mod binding;
mod constant;
pub mod dataflow;
mod dead;
pub(crate) mod decision;
mod defuse;
pub mod display;
pub mod escape;
mod expr;
pub mod fragment;
pub mod functionalize;
pub mod hoist;
pub mod lint;
mod liveness;
mod narrow;
mod pattern;
pub mod region;
mod regularize;
mod return_incref;
pub mod solve;
pub(crate) mod spec;
pub mod symbols;
pub mod tailcall;
#[cfg(test)]
pub(crate) mod testkit;
pub mod typeinfer;
pub mod types;

pub use analyze::{classify_form, AnalysisResult, Analyzer, FileForm, LambdaDecl};
pub use arena::{BindingArena, BindingInner, BindingScope};
pub use binding::{Binding, CaptureInfo, CaptureKind};
pub use constant::constant_value;
pub use dataflow::{analyze_dataflow, format_dataflow, DataflowInfo};
pub use defuse::ValueOrigin;
pub use escape::{analyze_escape, EscapeInfo};
pub use expr::{
    reset_hir_ids, BlockId, CallArg, Hir, HirId, HirKind, IntrinsicOp, Lambda, ParamBound,
    VarargKind,
};
pub use fragment::{FragmentBinding, HirFragment};
pub use lint::HirLinter;
pub use liveness::BitSet;
pub use pattern::{HirPattern, PatternBindings, PatternKey, PatternLiteral};
pub use region::infer::{analyze_regions, analyze_regions_with, format_regions};
pub(crate) use region::infer::{captured_bindings, return_frontier_regions};
pub use region::{CallClassification, Region, RegionInfo};
#[cfg(test)]
pub(crate) use regularize::lift;
pub(crate) use regularize::regularize;
pub(crate) use return_incref::return_minted_tail_calls;
pub use symbols::extract_symbols_from_hir;
pub use typeinfer::TypeInfo;
