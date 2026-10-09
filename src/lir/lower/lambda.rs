// audited: 2026-10-06
//! Lambda lowering: closure construction, and the body compiled as a function of its own.
//!
//! src/lir/lower/AGENTS.md
//!
//! Split by concern:
//! - [`expr`] — closure construction: capture collection, `MakeClosure`, and
//!   the capture-adopt region accounting.
//! - [`body`] — body compilation: per-function state save/restore, env layout,
//!   and lowering the body into a frozen function of its own.
//!
//! Both are inherent methods on `Lowerer`, so they need no re-export.

mod body;
mod expr;
