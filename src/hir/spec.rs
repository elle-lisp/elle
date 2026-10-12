// audited: 2026-10-05
//! A signal spec written as a literal in HIR: a keyword, or a set literal of keywords.
//!
//! docs/signals/inference.md

use super::arena::BindingArena;
use super::expr::{Hir, HirKind};
use crate::value::fiber::SignalBits;
use crate::value::SymbolId;

/// The bits a literal spec names, or `None` when `hir` is not one.
///
/// A literal is a keyword, or a set literal, which analysis turns into a call
/// to the `set` primitive with keyword arguments. The callee must be the
/// primitive itself: a binding that shadows `set` builds whatever it builds.
/// A keyword the registry does not know, a spliced argument, and anything
/// computed answer `None`.
pub(crate) fn literal_signal_bits(hir: &Hir, arena: &BindingArena) -> Option<SignalBits> {
    let bits_of = |name: &str| {
        crate::signals::registry::with_registry(|r| r.lookup(name)).map(SignalBits::from_bit)
    };
    match &hir.kind {
        HirKind::Keyword(name) => bits_of(name),
        HirKind::Call { func, args, .. } => {
            let HirKind::Var(binding) = &func.kind else {
                return None;
            };
            let inner = arena.get(*binding);
            if !inner.is_primitive || inner.name != SymbolId::of("set") {
                return None;
            }
            args.iter()
                .try_fold(SignalBits::EMPTY, |bits, arg| match &arg.expr.kind {
                    HirKind::Keyword(name) if !arg.spliced => Some(bits.union(bits_of(name)?)),
                    _ => None,
                })
        }
        _ => None,
    }
}
