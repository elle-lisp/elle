// audited: 2026-10-13
//! The cross-unit registries as one compile sees them: the standard library's compile records, and every other compile reads.
//!
//! docs/impl/dissolution/inline.md

use super::{DispatchWrapperRegistry, FnInlineRegistry};

/// The two registries a compile is handed. The variant decides whether the
/// compile records into them, so no other compile can.
pub enum Registries<'a> {
    /// The standard library's compile, whose definitions become the
    /// instance's globals.
    Record {
        dispatch: &'a mut DispatchWrapperRegistry,
        inline: &'a mut FnInlineRegistry,
    },
    /// Every other compile.
    Read {
        dispatch: &'a DispatchWrapperRegistry,
        inline: &'a FnInlineRegistry,
    },
}

impl Registries<'_> {
    /// The container-dispatch wrappers the standard library defines.
    pub(crate) fn dispatch(&self) -> &DispatchWrapperRegistry {
        match self {
            Registries::Record { dispatch, .. } => dispatch,
            Registries::Read { dispatch, .. } => dispatch,
        }
    }

    /// The inlineable function bodies the standard library defines.
    pub(crate) fn inline(&self) -> &FnInlineRegistry {
        match self {
            Registries::Record { inline, .. } => inline,
            Registries::Read { inline, .. } => inline,
        }
    }
}
