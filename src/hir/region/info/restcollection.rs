// audited: 2026-09-15
//! One collection a rest pattern builds, and the names that reach it.
//!
//! docs/impl/region/anchors.md

use super::super::Region;
use crate::hir::binding::Binding;

/// A collection a rest pattern BUILDS — `ArrayMutSliceFrom`'s fresh array or
/// `StructRest`'s fresh struct — and the names of the program that reach it.
///
/// The two names a collection can have answer different questions, so they are
/// recorded apart rather than read back off a pattern the consumer no longer
/// has. The tail-call exemption asks what a name HOLDS: the collection is the
/// rest name's own value, and it is nothing a projected name's value is. Every
/// other consumer asks which names must not outlive it, which is all of them.
#[derive(Debug, Clone)]
pub struct RestCollection {
    /// The phantom placeholder the solver minted for the collection. Its
    /// release loads the slot the lowerer parks the collection in.
    pub region: Region,
    /// The rest name whose VALUE is the collection, where a bare name matched
    /// the rest. `None` where a further pattern matched it.
    pub bound_name: Option<Binding>,
    /// Every name that reaches the collection: the `bound_name` alone, or each
    /// name the further pattern binds.
    pub holders: Vec<Binding>,
}

impl RestCollection {
    /// The collection a bare rest name holds. It is the one holder, and the
    /// collection is its value.
    pub fn bound(region: Region, name: Binding) -> Self {
        RestCollection {
            region,
            bound_name: Some(name),
            holders: vec![name],
        }
    }

    /// The collection a further pattern matched. Each name it binds projects
    /// the collection, so each must not outlive it and none of them holds it.
    pub fn projected(region: Region, holders: Vec<Binding>) -> Self {
        RestCollection {
            region,
            bound_name: None,
            holders,
        }
    }
}
