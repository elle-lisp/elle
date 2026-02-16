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
mod binding;
mod expr;
mod pattern;

pub use analyze::{AnalysisContext, AnalysisResult, Analyzer};
pub use binding::{BindingId, BindingInfo, BindingKind, CaptureInfo, CaptureKind};
pub use expr::{Hir, HirKind};
pub use pattern::{HirPattern, PatternBindings, PatternLiteral};
