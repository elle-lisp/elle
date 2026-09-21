// audited: 2026-09-21
//! The boot configuration: dump a booted instance's exports and macros as one
//! image, and install one into a fresh instance.
//!
//! docs/impl/image/boot.md
//!
//! The root is one immutable struct, so the manifest is body data and the
//! ordinary dumper and hydrator carry it. `BootImage` is the warm cache's
//! policy: where an instance looks for an image, and where it stores the one
//! it compiled.

mod build;
mod cache;
mod install;
#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::hir::region::RuntimeRegion;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::value::fiberheap::FiberHeap;
use crate::value::{TableKey, Value};

use super::ImageError;

pub(crate) use cache::store;

/// The keys the root struct and a macro entry carry. Both halves name them —
/// [`build`] writing, [`install`] reading — so they are spelled once.
mod key {
    pub(super) const SOURCES: &str = "sources";
    pub(super) const CORE: &str = "core";
    pub(super) const STDLIB: &str = "stdlib";
    pub(super) const MACROS: &str = "macros";
    pub(super) const PARAMS: &str = "params";
    pub(super) const OPTIONAL: &str = "optional";
    pub(super) const REST: &str = "rest";
    pub(super) const TEMPLATE: &str = "template";
    pub(super) const TRANSFORMER: &str = "transformer";
}

/// Where an instance looks for its boot image, and where it stores one.
///
/// A construction parameter rather than process state, for the reason
/// [`crate::compiler::stdlib_cache::StdlibCache`] is one: the suite builds many
/// instances across threads, and a directory that travels with the instance is
/// what keeps one test's cache invisible to the test beside it.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub enum BootImage {
    /// Never read or write an image; compile core, prelude and stdlib.
    #[default]
    Off,
    /// The process-wide choice: `boot` beneath the `--cache=<dir>` directory.
    Process,
    /// This directory, whatever the process-wide choice is.
    Dir(PathBuf),
}

/// Where a runtime's boot state came from. A warm cache that silently never
/// hits still yields a working runtime, so behaviour alone cannot tell the two
/// apart — this is what a caller reads instead.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BootSource {
    /// Hydrated from a boot image.
    Image,
    /// Compiled from core.lisp, prelude.lisp and stdlib.lisp.
    Compiled,
}

/// The digest of the three sources this binary boots from.
///
/// The fingerprint gates layout and says nothing about the sources, so this is
/// what decides whether an image still describes them
/// (docs/impl/image/boot.md). It also names the warm cache's file, so an edit
/// to any of the three misses rather than loads.
pub fn sources_digest() -> String {
    use crate::pipeline::sources::{CORE, PRELUDE, STDLIB};
    use std::hash::{Hash, Hasher};
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    for source in [CORE, PRELUDE, STDLIB] {
        source.hash(&mut hasher);
    }
    format!("{:016x}", hasher.finish())
}

/// A hydrated boot image, installed as a process root and ready for a fresh
/// instance to read its pieces out of.
///
/// The three fields are read out of the root struct once, here, rather than on
/// every question: a borrow of the macro table is then the `Boot`'s, which is
/// the region's lifetime rather than a temporary's.
#[derive(Debug)]
pub struct Boot {
    /// The root struct, whose region holds the whole boot graph.
    root: Value,
    /// core.lisp's export struct.
    core: Value,
    /// stdlib.lisp's export struct.
    stdlib: Value,
    /// The macro table: each macro's name to its entry struct.
    macros: Value,
    /// The region the pages were mapped into, kept for the free a refused
    /// install owes.
    region: RuntimeRegion,
    /// One past the highest hygiene scope counter the image's templates carry.
    pub(crate) scope_watermark: u32,
}

impl Boot {
    /// core.lisp's export struct.
    pub(crate) fn core_exports(&self) -> Value {
        self.core
    }

    /// stdlib.lisp's export struct.
    pub(crate) fn stdlib_exports(&self) -> Value {
        self.stdlib
    }

    /// The macro table's entries, in the sorted order the dump wrote them.
    pub(crate) fn macro_entries(&self) -> &[(TableKey, Value)] {
        self.macros.as_struct().unwrap_or(&[])
    }

    /// Define every macro the image carries on `expander`, and raise its scope
    /// counter past the scopes the image's syntax carries
    /// (docs/impl/image/format.md).
    pub(crate) fn install_macros(
        &self,
        heap: &mut FiberHeap,
        expander: &mut Expander,
        symbols: &SymbolTable,
    ) {
        install::macros(heap, self, expander, symbols);
        expander.raise_scope_counter(self.scope_watermark);
    }
}

/// The value a struct holds under the keyword `name`, or `None`.
///
/// A struct's entries are sorted by key, so this is the binary search a `get`
/// on the same value would run.
pub(super) fn field(structure: Value, name: &str) -> Option<Value> {
    let entries = structure.as_struct()?;
    let want = TableKey::keyword(name);
    entries
        .binary_search_by(|(k, _)| k.cmp(&want))
        .ok()
        .map(|i| entries[i].1)
}

/// The text a struct's `name` field holds, owned.
///
/// Owned because the borrow `as_str` hands out is the `Value`'s rather than
/// the region's, and every caller here holds the `Value` in a temporary.
pub(super) fn field_text(structure: Value, name: &str) -> Option<String> {
    let value = field(structure, name)?;
    value.as_str().map(str::to_owned)
}

/// Hydrate the boot image at `path` into `heap`, or answer why this binary
/// refuses it.
///
/// A refusal leaves no region behind. On success the image's region is a
/// process root, so the whole boot graph is released by one decref at teardown
/// — the caller installs the pieces and never frees them itself.
pub fn hydrate_path(
    heap: &mut FiberHeap,
    symbols: &mut SymbolTable,
    path: &Path,
) -> Result<Boot, ImageError> {
    let hydrated = super::hydrate_path(heap, symbols, path)?;
    let nil_field = |name| field(hydrated.root, name).unwrap_or(Value::NIL);
    let boot = Boot {
        root: hydrated.root,
        core: nil_field(key::CORE),
        stdlib: nil_field(key::STDLIB),
        macros: nil_field(key::MACROS),
        region: hydrated.region,
        scope_watermark: hydrated.scope_watermark,
    };
    match boot.check(symbols) {
        Ok(()) => {
            crate::value::arena::register_process_root_region(heap, boot.region);
            Ok(boot)
        }
        Err(e) => {
            // The mint's reference is this call's, and nothing else has taken
            // one, so releasing it frees the mapping the load will not use.
            heap.decref_region_if_present(boot.region);
            Err(e)
        }
    }
}

impl Boot {
    /// Whether this image describes the sources this binary carries, and holds
    /// the four fields an install reads.
    fn check(&self, symbols: &SymbolTable) -> Result<(), ImageError> {
        let corrupt = |what: &str| Err(ImageError::Corrupt(what.into()));
        let Some(found) = field_text(self.root, key::SOURCES) else {
            return corrupt("a boot image whose root carries no source digest");
        };
        let expected = sources_digest();
        if found != expected {
            return Err(ImageError::Sources { expected, found });
        }
        if self.core.as_struct().is_none() {
            return corrupt("a boot image whose core exports are not a struct");
        }
        if self.stdlib.as_struct().is_none() {
            return corrupt("a boot image whose stdlib exports are not a struct");
        }
        if self.macros.as_struct().is_none() {
            return corrupt("a boot image whose macro table is not a struct");
        }
        install::check_names(self, symbols)
    }
}

/// Dump `cctx`'s boot state to `path`, atomically.
///
/// The graph is the two export structs and the macro table, under a digest of
/// the sources they came from. The aggregate is assembled in a scratch region
/// dropped either way: it names values the instance owns and takes no
/// reference to them, exactly as the dumper's own scratch does.
pub(crate) fn dump(
    heap: &mut FiberHeap,
    symbols: &mut SymbolTable,
    cctx: &crate::pipeline::CompileCtx,
    path: &Path,
) -> Result<(), ImageError> {
    let scratch = heap.new_runtime_region();
    let result = build::root(heap, symbols, cctx, scratch)
        .and_then(|root| super::dump(heap, symbols, root, path));
    heap.decref_region_if_present(scratch);
    result
}
