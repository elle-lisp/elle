// audited: 2026-10-12
//! The compile registries a boot image carries in its `:compiler` field, and the bytes that hold them.
//!
//! docs/impl/image/boot.md

use serde::{Deserialize, Serialize};

use crate::hir::region::RuntimeRegion;
use crate::hir::typeinfer::{StoredDispatchRegistry, StoredFnInlineRegistry};
use crate::pipeline::CompileCtx;
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::{build, Value};

use super::super::ImageError;
use super::{field, key};

/// The two cross-unit compile registries, in the form `:compiler` holds.
///
/// Each snapshot is in name order (`to_stored` sorts it), so one boot state
/// encodes to one byte string.
#[derive(Default, Serialize, Deserialize)]
pub(super) struct Registries {
    pub(super) dispatch_wrappers: StoredDispatchRegistry,
    pub(super) fn_inline: StoredFnInlineRegistry,
}

impl std::fmt::Debug for Registries {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Registries")
            .field("dispatch_wrappers", &self.dispatch_wrappers.by_name.len())
            .field("fn_inline", &self.fn_inline.by_name.len())
            .finish()
    }
}

impl Registries {
    /// A snapshot of `cctx`'s two registries.
    pub(super) fn of(cctx: &CompileCtx, symbols: &SymbolTable) -> Registries {
        let (dispatch_wrappers, fn_inline) = cctx.compile_registries();
        Registries {
            dispatch_wrappers: dispatch_wrappers.to_stored(symbols),
            fn_inline: fn_inline.to_stored(symbols),
        }
    }

    /// The bytes `:compiler` holds.
    pub(super) fn encode(&self) -> Vec<u8> {
        // bincode into a `Vec` fails only where a serializer refuses, and the
        // one that can is a heap `Value`'s. A fragment never holds a `Value`:
        // its body admits literals, references, calls and control forms, and
        // declines a quote.
        bincode::serialize(self).expect("a compile registry holds no heap value")
    }

    /// The registries `bytes` encode, or why they do not decode.
    pub(super) fn decode(bytes: &[u8]) -> Result<Registries, ImageError> {
        bincode::deserialize(bytes).map_err(|e| {
            ImageError::Corrupt(format!(
                "a boot image whose compile registries do not decode: {e}"
            ))
        })
    }

    /// Replace `cctx`'s two registries with these, re-interning every name in
    /// `symbols`.
    pub(super) fn restore(self, cctx: &mut CompileCtx, symbols: &mut SymbolTable) {
        let (dispatch_wrappers, fn_inline) = cctx.compile_registries_mut();
        dispatch_wrappers.restore(self.dispatch_wrappers, symbols);
        fn_inline.restore(self.fn_inline, symbols);
    }
}

/// The `:compiler` value a dump writes: `cctx`'s registries, encoded, as
/// immutable bytes in `region`.
pub(super) fn value(
    heap: &mut FiberHeap,
    symbols: &SymbolTable,
    cctx: &CompileCtx,
    region: RuntimeRegion,
) -> Value {
    build::bytes(heap, Registries::of(cctx, symbols).encode(), region)
}

/// The registries a hydrated root's `:compiler` field holds, or why the image
/// is refused.
pub(super) fn read(root: Value) -> Result<Registries, ImageError> {
    let corrupt = |what: &str| Err(ImageError::Corrupt(what.into()));
    let Some(registries) = field(root, key::COMPILER) else {
        return corrupt("a boot image whose root carries no compile registries");
    };
    let Some(bytes) = registries.as_bytes() else {
        return corrupt("a boot image whose compile registries are not bytes");
    };
    Registries::decode(bytes)
}
