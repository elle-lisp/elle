// audited: 2026-10-21
//! The names a fragment holds where no value holds them, which a carrier spells in another instance.
//!
//! docs/impl/region/template.md

use super::{pattern, FragmentBinding, HirFragment};
use crate::hir::expr::{Hir, HirKind};
use crate::value::SymbolId;

/// A name a fragment holds where no value holds it: a symbol by its id, or a
/// keyword by the spelling the body wrote.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum FragmentName<'a> {
    Symbol(SymbolId),
    Keyword(&'a str),
}

impl HirFragment {
    /// Call `f` on every name the fragment holds: each binding's, each global,
    /// and each keyword and symbol key its body spells. A carrier that moves the
    /// fragment to another instance spells them there.
    pub fn for_each_name(&self, f: &mut impl FnMut(FragmentName<'_>)) {
        for entry in &self.bindings {
            f(FragmentName::Symbol(match entry {
                FragmentBinding::Local(inner) => inner.name,
                FragmentBinding::Global(name) => *name,
            }));
        }
        body_names(&self.body, f);
    }
}

/// Call `f` on every name `h` spells outside the binding table. Of the forms
/// `rebind` admits, a keyword literal and a `match` pattern hold one; a form
/// added there that holds a name has to be added here too.
fn body_names(h: &Hir, f: &mut impl FnMut(FragmentName<'_>)) {
    match &h.kind {
        HirKind::Keyword(k) => f(FragmentName::Keyword(k)),
        HirKind::Match { arms, .. } => {
            for (pat, _, _) in arms {
                pattern::pattern_names(pat, f);
            }
        }
        _ => {}
    }
    h.for_each_child(|c| body_names(c, f));
}
