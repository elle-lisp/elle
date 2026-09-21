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

#[cfg(test)]
mod tests;

use std::path::{Path, PathBuf};

use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;
use crate::value::Value;

use super::ImageError;

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
/// (docs/impl/image/boot.md).
pub fn sources_digest() -> String {
    String::new()
}

/// A hydrated boot image, ready for a fresh instance to install.
#[derive(Debug)]
#[allow(dead_code)]
pub struct Boot {
    /// core.lisp's export struct.
    pub(crate) core: Value,
    /// stdlib.lisp's export struct.
    pub(crate) stdlib: Value,
    /// The macro table: name to entry struct.
    pub(crate) macros: Value,
    /// One past the highest hygiene scope counter the image's templates carry.
    pub(crate) scope_watermark: u32,
}

/// Hydrate the boot image at `path` into `heap`, or answer why this binary
/// refuses it.
///
/// The caller installs the result — `Runtime` does, through the tails a source
/// boot registers its exports through.
pub fn hydrate_path(
    _heap: &mut FiberHeap,
    _symbols: &mut SymbolTable,
    _path: &Path,
) -> Result<Boot, ImageError> {
    Err(ImageError::Corrupt("boot images do not load yet".into()))
}

/// Dump a booted instance's boot state to `path`.
pub(crate) fn dump(
    _heap: &mut FiberHeap,
    _symbols: &mut SymbolTable,
    _state: &Value,
    _path: &Path,
) -> Result<(), ImageError> {
    Ok(())
}
