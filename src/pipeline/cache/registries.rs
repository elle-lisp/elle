// audited: 2026-10-13
//! The instance's cross-unit compile registries, and the one compile that records into them.
//!
//! docs/impl/dissolution/inline.md

use crate::hir::typeinfer::{DispatchWrapperRegistry, FnInlineRegistry, Registries};

/// The registries every compile in an instance reads. The standard library's
/// compile alone records into them, while `CompileCtx::compiling_stdlib` runs
/// it.
///
/// Compile-time-only state: the rewrites they drive leave the direct op or the
/// inlined body in the HIR, so nothing here reaches the VM.
#[derive(Default)]
pub(super) struct InstanceRegistries {
    /// Container-dispatch wrappers by name, such as `push` and `put`
    /// (`monomorphize.rs`).
    pub(super) dispatch: DispatchWrapperRegistry,
    /// Inlineable function bodies by name, such as `inc` and `dec` (`fuse.rs`).
    pub(super) inline: FnInlineRegistry,
    /// Set while the standard library compiles.
    pub(super) stdlib_compile: bool,
}

impl InstanceRegistries {
    /// The registries as the compile now running sees them.
    pub(super) fn view(&mut self) -> Registries<'_> {
        if self.stdlib_compile {
            Registries::Record {
                dispatch: &mut self.dispatch,
                inline: &mut self.inline,
            }
        } else {
            Registries::Read {
                dispatch: &self.dispatch,
                inline: &self.inline,
            }
        }
    }
}
