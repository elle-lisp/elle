// audited: 2026-09-05
//! The bindings a pattern binds to a BORROWED subview of its scrutinee, collected
//! once over the whole HIR before any body is lowered.
//!
//! docs/impl/region/rules.md

use super::*;

impl Lowerer<'_> {
    /// Collect, over the whole HIR, the bindings each `match`/`destructure`
    /// pattern binds to a BORROWED subview of its scrutinee — the structural
    /// element positions (`First`/`Rest`/`Index`/`Key`), excluding fresh-owned
    /// collectors (`Slice`/`StructRest`, the `& rest` of array/struct patterns).
    ///
    /// Runs once at `lower()` entry, before any body lowering, so a `match`
    /// compound in tail-argument position has its aliases registered before the
    /// call site classifies that argument. `Hir::for_each_child` deliberately
    /// does NOT enumerate pattern bindings (patterns are not HIR children), so
    /// this walk enumerates `Match` arms / `Destructure` patterns itself and
    /// recurses through `for_each_child` for the ordinary expression tree.
    pub(super) fn precompute_destructure_aliases(&mut self, hir: &Hir) {
        match &hir.kind {
            HirKind::Match { arms, .. } => {
                for (pat, _, _) in arms {
                    collect_pattern_aliases(pat, &mut self.destructure_alias_bindings, false);
                }
            }
            HirKind::Destructure { pattern, .. } => {
                collect_pattern_aliases(pattern, &mut self.destructure_alias_bindings, false);
            }
            _ => {}
        }
        hir.for_each_child(|c| self.precompute_destructure_aliases(c));
    }
}

/// Collect the bindings one pattern binds to a BORROWED subview of its
/// scrutinee into `out` — the structural ELEMENT positions, mirroring
/// `access_is_borrowed_element`'s access-path gate over the source pattern
/// shape. `in_element` is true when this pattern is itself reached through an
/// element load (so a bare `Var` at that position is an element binding, while
/// a `Var` pattern at the top bound to the whole scrutinee is not).
///
/// Fresh-owned collectors are excluded: the `& rest` of a `Tuple`/`Array`
/// compiles to `Slice` (a new owned array), and of a `Struct`/`Table` to
/// `StructRest` (a new owned struct) — a binding reached under one owns its
/// new container. The cons `rest` of a `List`/`Pair` is NOT excluded: it is
/// `AccessPath::Rest`, a borrowed view into the scrutinee's cells.
fn collect_pattern_aliases(
    pat: &HirPattern,
    out: &mut std::collections::HashSet<Binding>,
    in_element: bool,
) {
    match pat {
        HirPattern::Var(b) => {
            if in_element {
                out.insert(*b);
            }
        }
        HirPattern::Pair { head, tail } => {
            // head = First, tail = Rest — both borrowed cell views.
            collect_pattern_aliases(head, out, true);
            collect_pattern_aliases(tail, out, true);
        }
        HirPattern::List { elements, rest } => {
            // list element N = First of N rests — borrowed; the cons rest
            // itself is AccessPath::Rest — borrowed.
            for e in elements {
                collect_pattern_aliases(e, out, true);
            }
            if let Some(r) = rest {
                collect_pattern_aliases(r, out, true);
            }
        }
        HirPattern::Tuple { elements, rest } | HirPattern::Array { elements, rest } => {
            // array element = Index — borrowed. The & rest (Slice) mints a
            // fresh owned array; a binding under it owns the slice.
            for e in elements {
                collect_pattern_aliases(e, out, true);
            }
            let _ = rest; // slice rest: fresh-owned, not a borrower
        }
        HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
            // entry value = Key — borrowed. The & rest (StructRest) mints a
            // fresh owned struct; a binding under it owns the rest-struct.
            for (_, v) in entries {
                collect_pattern_aliases(v, out, true);
            }
            let _ = rest; // struct rest: fresh-owned, not a borrower
        }
        HirPattern::NamedStruct { entries } => {
            for (_, v) in entries {
                collect_pattern_aliases(v, out, true);
            }
        }
        HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
            // A set pattern binds the WHOLE value (type-guard only) — Root, not
            // an element — but if it sits inside an element load (e.g. an array
            // of sets), the set value itself is that element: pass in_element
            // through so a Var at that depth is marked when reached sideways.
            collect_pattern_aliases(binding, out, in_element);
        }
        HirPattern::Or(alternatives) => {
            // An or-pattern binds the same names in every arm, each reached
            // through the same access path — collect each arm identically.
            for alt in alternatives {
                collect_pattern_aliases(alt, out, in_element);
            }
        }
        HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
    }
}
