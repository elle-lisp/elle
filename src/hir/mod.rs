//! High-level Intermediate Representation (HIR)
//!
//! HIR is the fully-analyzed form produced from expanded Syntax. All bindings
//! are resolved, effects are inferred, and captures are computed. This is the
//! input to the lowering phase that produces LIR.
//!
//! Pipeline:
//! ```text
//! Syntax → Expand → Syntax → Analyze → HIR → Lower → LIR → Emit → Bytecode
//! ```

mod analyze;
pub mod binding;
mod expr;
pub mod lint;
mod pattern;
pub mod symbols;
pub mod tailcall;

pub use analyze::{AnalysisResult, Analyzer};
pub use binding::{Binding, CaptureInfo, CaptureKind};
pub use expr::{Hir, HirKind};
pub use lint::HirLinter;
pub use pattern::{HirPattern, PatternBindings, PatternLiteral};
pub use symbols::extract_symbols_from_hir;
