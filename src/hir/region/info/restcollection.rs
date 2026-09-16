// audited: 2026-09-16
//! One collection a rest pattern builds, and the names that reach it.
//!
//! docs/impl/region/anchors.md

use super::super::Region;
use crate::hir::binding::Binding;

/// A collection a rest pattern BUILDS — `ArrayMutSliceFrom`'s fresh array or
/// `StructRest`'s fresh struct — and every name of the program that reaches it.
///
/// A record rather than a name-and-region pair, because the number of names is
/// not one: a collection a further pattern matched has several, and one the
/// program only passes through has none. The consumers ask about the
/// collection either way — which names must not outlive it, and whether a given
/// name reaches one at all.
#[derive(Debug, Clone)]
pub struct RestCollection {
    /// The phantom placeholder the solver minted for the collection. Its
    /// release loads the slot the lowerer parks the collection in.
    pub region: Region,
    /// Every name that reaches the collection: the rest name whose value it is,
    /// or each name the further pattern that matched the rest binds. Empty
    /// where no name reaches it, which leaves the release on its base pin.
    pub holders: Vec<Binding>,
}

impl RestCollection {
    /// The collection `holders` reach, and that no other name does.
    pub fn new(region: Region, holders: Vec<Binding>) -> Self {
        RestCollection { region, holders }
    }
}
