//! Lambda lowering: closure construction and body compilation.
//!
//! Split by concern:
//! - [`expr`] — closure construction: capture collection, `MakeClosure`, and
//!   the capture-adopt region accounting.
//! - [`body`] — body compilation: per-function state save/restore, env layout,
//!   and lowering the body into a self-contained `LirFunction`.
//!
//! Both are inherent methods on `Lowerer`, so they need no re-export; module
//! declarations alone keep `crate::lir::lower::lambda::*` resolving unchanged.

mod body;
mod expr;
