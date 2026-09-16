// audited: 2026-09-16
//! What a rest sub-pattern builds when it is lowered, and which of the
//! pattern's names reach the collection it built.
//!
//! docs/impl/region/anchors.md
//! docs/destructuring.md

use super::{Binding, HirPattern};

impl HirPattern {
    /// Does lowering THIS pattern's own rest build a fresh collection?
    ///
    /// A rest of a sequence or keyed pattern lowers to `ArrayMutSliceFrom` or
    /// `StructRest`, each of which builds one. A `_` rest reads nothing out of
    /// what it built, so the build has no consumer at all and no lowering emits
    /// it — except that `ArrayMutSliceFrom` also CHECKS that the scrutinee is an
    /// array, which a sequence pattern with no fixed element makes nowhere else.
    /// `StructRest` signals on no input, so a keyed pattern always skips.
    ///
    /// Every lowering of a rest asks this before it emits the build, and
    /// [`Self::building_rests`] counts with it. A disagreement leaves a
    /// collection with no release route, or every later index naming the wrong
    /// allocation.
    ///
    /// docs/impl/region/anchors.md
    pub fn own_rest_builds(&self) -> bool {
        match self {
            HirPattern::Tuple { elements, rest } | HirPattern::Array { elements, rest } => rest
                .as_deref()
                .is_some_and(|r| r.reads_its_collection() || elements.is_empty()),
            HirPattern::Struct { rest, .. } | HirPattern::Table { rest, .. } => rest
                .as_deref()
                .is_some_and(HirPattern::reads_its_collection),
            // A `List` rest is the remaining cons tail, and every other shape
            // has no rest at all.
            _ => false,
        }
    }

    /// Does this rest sub-pattern read the collection a build would hand it?
    ///
    /// A `_` binds no name and tests nothing, so the collection would be
    /// garbage from birth. Every other sub-pattern either holds the collection
    /// or projects it.
    fn reads_its_collection(&self) -> bool {
        !matches!(self, HirPattern::Wildcard)
    }

    /// Every rest sub-pattern whose lowering BUILDS a fresh collection, in the
    /// order the lowerer reaches them.
    ///
    /// `lower_destructure` emits one build per entry here and takes the n-th
    /// placeholder at the n-th build, so an index into this list names the same
    /// allocation on both sides.
    ///
    /// docs/impl/region/anchors.md
    pub fn building_rests(&self) -> Vec<&HirPattern> {
        let mut out = Vec::new();
        self.collect_building_rests(&mut out);
        out
    }

    /// Every name bound DIRECTLY by a rest whose lowering builds a fresh
    /// collection, in the order the lowerer reaches them.
    ///
    /// "Directly" excludes a rest matched by a further pattern, whose names
    /// project the collection rather than hold it. The decision tree reads this
    /// set, reaching a rest name through an access path whose outermost step is
    /// the build itself; it re-runs that step per path, so it can key nothing
    /// on the sub-pattern the way `lower_destructure` can.
    ///
    /// docs/impl/region/anchors.md
    pub fn allocating_rest_bindings(&self) -> Vec<Binding> {
        self.building_rests()
            .into_iter()
            .filter_map(|r| match r {
                HirPattern::Var(b) => Some(*b),
                _ => None,
            })
            .collect()
    }

    /// The names that reach the collection this pattern is the rest of: every
    /// name it binds, stopping at a nested rest that builds a collection of its
    /// own, whose names reach that one instead.
    ///
    /// The binding chain carries the collection's release over each of these
    /// names' uses, so a name left out is a release that lands before its last
    /// read. The set is empty where the sub-pattern binds no name of its own,
    /// and the collection is then released at its base pin.
    ///
    /// docs/impl/region/anchors.md
    pub fn rest_collection_holders(&self) -> Vec<Binding> {
        let mut out = Vec::new();
        self.collect_rest_collection_holders(&mut out);
        out
    }

    fn collect_building_rests<'p>(&'p self, out: &mut Vec<&'p HirPattern>) {
        // Sub-patterns first, then this pattern's own rest, then whatever the
        // rest itself contains: the order every lowering path reaches them in,
        // so an index into the result names the same allocation on both sides.
        match self {
            HirPattern::Pair { head, tail } => {
                head.collect_building_rests(out);
                tail.collect_building_rests(out);
            }
            // A `List` rest is the remaining cons tail, so it builds nothing
            // here. Its sub-patterns still can.
            HirPattern::List { elements, rest } => {
                for p in elements {
                    p.collect_building_rests(out);
                }
                if let Some(r) = rest {
                    r.collect_building_rests(out);
                }
            }
            HirPattern::Tuple { elements, rest } | HirPattern::Array { elements, rest } => {
                for p in elements {
                    p.collect_building_rests(out);
                }
                // A rest the lowerer skips is not reached at all, its own
                // sub-pattern included — which is what keeps this list and the
                // emission in step.
                if let Some(r) = rest.as_deref().filter(|_| self.own_rest_builds()) {
                    out.push(r);
                    r.collect_building_rests(out);
                }
            }
            HirPattern::Struct { entries, rest } | HirPattern::Table { entries, rest } => {
                for (_, p) in entries {
                    p.collect_building_rests(out);
                }
                if let Some(r) = rest.as_deref().filter(|_| self.own_rest_builds()) {
                    out.push(r);
                    r.collect_building_rests(out);
                }
            }
            HirPattern::NamedStruct { entries } => {
                for (_, p) in entries {
                    p.collect_building_rests(out);
                }
            }
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
                binding.collect_building_rests(out)
            }
            // Every alternative, not just the first: an or-pattern's cases bind
            // the same NAMES, and the arena gives each case its own `Binding`.
            HirPattern::Or(alternatives) => {
                for alt in alternatives {
                    alt.collect_building_rests(out);
                }
            }
            HirPattern::Wildcard
            | HirPattern::Nil
            | HirPattern::Literal(_)
            | HirPattern::Var(_) => {}
        }
    }

    fn collect_rest_collection_holders(&self, out: &mut Vec<Binding>) {
        match self {
            HirPattern::Var(b) => out.push(*b),
            HirPattern::Pair { head, tail } => {
                head.collect_rest_collection_holders(out);
                tail.collect_rest_collection_holders(out);
            }
            // A `List` rest builds nothing, so the names beneath it reach the
            // same collection the elements' names do.
            HirPattern::List { elements, rest } => {
                for p in elements {
                    p.collect_rest_collection_holders(out);
                }
                if let Some(r) = rest {
                    r.collect_rest_collection_holders(out);
                }
            }
            // The rest of one of these BUILDS, so the names beneath it reach
            // that collection instead and the descent stops.
            HirPattern::Tuple { elements, .. } | HirPattern::Array { elements, .. } => {
                for p in elements {
                    p.collect_rest_collection_holders(out);
                }
            }
            HirPattern::Struct { entries, .. } | HirPattern::Table { entries, .. } => {
                for (_, p) in entries {
                    p.collect_rest_collection_holders(out);
                }
            }
            HirPattern::NamedStruct { entries } => {
                for (_, p) in entries {
                    p.collect_rest_collection_holders(out);
                }
            }
            HirPattern::Set { binding } | HirPattern::SetMut { binding } => {
                binding.collect_rest_collection_holders(out)
            }
            HirPattern::Or(alternatives) => {
                for alt in alternatives {
                    alt.collect_rest_collection_holders(out);
                }
            }
            HirPattern::Wildcard | HirPattern::Nil | HirPattern::Literal(_) => {}
        }
    }
}
