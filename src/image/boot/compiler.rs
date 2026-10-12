// audited: 2026-10-12
//! The compile registries a boot image carries in its `:compiler` field, and the bytes that hold them.
//!
//! docs/impl/image/boot.md

use crate::hir::typeinfer::{StoredDispatchRegistry, StoredFnInlineRegistry};
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;

use super::super::ImageError;

/// The two cross-unit compile registries, in the form `:compiler` holds.
#[derive(Default)]
pub(crate) struct Registries {
    pub(super) dispatch_wrappers: StoredDispatchRegistry,
    pub(super) fn_inline: StoredFnInlineRegistry,
}

impl Registries {
    /// A snapshot of `cctx`'s two registries.
    pub(crate) fn of(_cctx: &CompileCtx, _symbols: &SymbolTable) -> Registries {
        Registries::default()
    }

    /// The bytes `:compiler` holds.
    pub(super) fn encode(&self) -> Vec<u8> {
        Vec::new()
    }

    /// The registries `bytes` encode, or why they do not decode.
    pub(super) fn decode(_bytes: &[u8]) -> Result<Registries, ImageError> {
        Err(ImageError::Corrupt("unimplemented".into()))
    }
}
