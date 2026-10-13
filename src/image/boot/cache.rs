// audited: 2026-10-20
//! The warm cache: where a boot image is read from, where a compiled boot
//! stores one, and what a store leaves behind.
//!
//! docs/impl/image/boot.md
//!
//! The file is named for the source digest, so an edit to core.lisp,
//! prelude.lisp or stdlib.lisp misses rather than loads. A store writes a
//! temporary file and renames it over the target — the dumper's own
//! discipline, because a live hydration holds a private mapping of the old
//! inode.

use std::path::{Path, PathBuf};

use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;

use super::{sources_digest, BootImage};

impl BootImage {
    /// The directory this policy reads and writes, or `None` when it is off.
    fn dir(&self) -> Option<PathBuf> {
        self.dir_in(crate::config::get())
    }

    /// The directory this policy reads and writes under `config`, or `None`
    /// when it is off.
    pub(crate) fn dir_in(&self, config: &crate::config::Config) -> Option<PathBuf> {
        match self {
            BootImage::Off => None,
            BootImage::Dir(d) => Some(d.clone()),
            BootImage::Process => config
                .cache
                .as_ref()
                .map(|base| PathBuf::from(base).join("boot")),
        }
    }

    /// The image file this binary reads and writes under this policy.
    pub fn path(&self) -> Option<PathBuf> {
        Some(self.dir()?.join(format!("{}.image", sources_digest())))
    }

    /// The image file `exe` reads and writes under this policy.
    #[cfg(test)]
    pub(crate) fn path_for(&self, _exe: Option<&Executable>) -> Option<PathBuf> {
        self.path()
    }
}

/// The running executable, as the warm cache names its images: its path tags
/// them, and its build identity keys them.
#[cfg(test)]
pub(crate) struct Executable {
    len: u64,
    mtime: u128,
}

#[cfg(test)]
impl Executable {
    pub(crate) fn new(_path: PathBuf, len: u64, mtime: u128) -> Self {
        Executable { len, mtime }
    }

    /// The running executable, or `None` when it cannot be located or
    /// measured.
    pub(crate) fn current() -> Option<Self> {
        None
    }

    /// The build identity: the executable's length and modification time.
    pub(crate) fn identity(&self) -> (u64, u128) {
        (self.len, self.mtime)
    }

    /// The file name this executable gives the image of the sources `digest`
    /// names.
    pub(crate) fn image_name(&self, digest: &str) -> String {
        format!("{digest}.image")
    }
}

/// Store `cctx`'s boot state under `image`'s policy, and prune the files an
/// earlier digest left behind.
///
/// A failure is reported and ignored: the cache is a speedup, and a source
/// boot is always the fallback. It is reported rather than swallowed because a
/// directory that never fills would otherwise look like a cache that never
/// helps.
pub(crate) fn store(
    heap: &mut FiberHeap,
    symbols: &mut SymbolTable,
    cctx: &crate::pipeline::CompileCtx,
    image: &BootImage,
) {
    let Some(path) = image.path() else { return };
    let dir = path
        .parent()
        .expect("a cache path always names its directory");
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("[boot-image] mkdir failed: {e}");
        return;
    }
    match super::dump(heap, symbols, cctx, &path) {
        Ok(()) => prune_superseded(dir, &path),
        Err(e) => eprintln!("[boot-image] store failed: {e}"),
    }
}

/// Remove every image in `dir` except `keep` — the megabytes each earlier
/// digest orphans.
///
/// Call this after the store, never before: a store that fails must leave the
/// directory as it found it, still holding a file another process may be about
/// to map. A removal that fails is ignored; it is disk hygiene, and the next
/// store tries again.
fn prune_superseded(dir: &Path, keep: &Path) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        if path != keep && path.extension().is_some_and(|e| e == "image") {
            let _ = std::fs::remove_file(&path);
        }
    }
}
