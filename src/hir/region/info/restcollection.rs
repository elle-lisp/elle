// audited: 2026-09-15
//! One collection a rest pattern builds, and the names that reach it.
//!
//! docs/impl/region/anchors.md

use super::super::Region;
use crate::hir::binding::Binding;

/// A collection a rest pattern BUILDS — `ArrayMutSliceFrom`'s fresh array or
/// `StructRest`'s fresh struct — and every name of the program that reaches it.
///
/// A record rather than a name-and-region pair, because a collection a further
/// pattern matched has several names and the consumers ask about the
/// collection: which names must not outlive it, and whether a given name
/// reaches one at all.
#[derive(Debug, Clone)]
pub struct RestCollection {
    /// The phantom placeholder the solver minted for the collection. Its
    /// release loads the slot the lowerer parks the collection in.
    pub region: Region,
    /// Every name that reaches the collection: the rest name whose value it is,
    /// or each name the further pattern that matched the rest binds.
    pub holders: Vec<Binding>,
}

impl RestCollection {
    /// The collection a bare rest name holds. It is the one name that reaches
    /// it, and the collection is its value.
    pub fn bound(region: Region, name: Binding) -> Self {
        RestCollection {
            region,
            holders: vec![name],
        }
    }

    /// The collection a further pattern matched. Each name it binds projects
    /// the collection, so each must not outlive it.
    pub fn projected(region: Region, holders: Vec<Binding>) -> Self {
        RestCollection { region, holders }
    }
}
