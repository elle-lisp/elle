// audited: 2026-11-22
//! The value a HIR node has at compile time, when the node is a constant.
//!
//! src/lir/lower/overview.md

use super::{Binding, Hir, HirKind};
use crate::value::Value;

/// The value `hir` has at compile time: a literal, the value a `Quote` holds,
/// or a read of a binding whose value `known` answers.
pub fn constant_value(hir: &Hir, known: impl Fn(&Binding) -> Option<Value>) -> Option<Value> {
    match &hir.kind {
        HirKind::Int(n) => Some(Value::int(*n)),
        HirKind::Float(f) => Some(Value::float(*f)),
        HirKind::Bool(b) => Some(Value::bool(*b)),
        HirKind::Nil => Some(Value::NIL),
        HirKind::Keyword(k) => Some(Value::keyword(k)),
        HirKind::Quote(v) => Some(*v),
        HirKind::Var(b) => known(b),
        _ => None,
    }
}
