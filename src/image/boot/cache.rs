// audited: 2026-10-12
//! The warm cache: where a boot image is read from, where a compiled boot
//! stores one, and what a store leaves behind.
//!
//! docs/impl/image/boot.md
//!
//! The file is named for the executable, the source digest and the binary's
//! identity, so an edit to a boot source misses and a rebuild never opens the
//! last build's image. A store writes a temporary file and renames it over the
//! target — the dumper's own discipline, because a live hydration holds a
//! private mapping of the old inode.

use std::hash::{Hash, Hasher};
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

    /// The image file this binary reads and writes under this policy, or
    /// `None` when the policy is off or the binary cannot identify itself.
    pub fn path(&self) -> Option<PathBuf> {
        self.path_for(Executable::current().as_ref())
    }

    /// The image file `exe` reads and writes under this policy. An executable
    /// that could not be measured gets none, rather than a name another build
    /// could share.
    pub(crate) fn path_for(&self, exe: Option<&Executable>) -> Option<PathBuf> {
        Some(self.dir()?.join(exe?.image_name(&sources_digest())))
    }
}

/// The running executable, as the warm cache names its images: its path tags
/// them, and its build identity keys them.
pub(crate) struct Executable {
    path: PathBuf,
    len: u64,
    mtime: u128,
}

impl Executable {
    pub(crate) fn new(len: u64, mtime: u128) -> Self {
        Executable {
            path: PathBuf::new(),
            len,
            mtime,
        }
    }

    /// The running executable, or `None` when it cannot be located or
    /// measured.
    pub(crate) fn current() -> Option<Self> {
        let path = std::env::current_exe().ok()?;
        let meta = std::fs::metadata(&path).ok()?;
        let mtime = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        Some(Executable {
            path,
            ..Executable::new(meta.len(), mtime)
        })
    }

    /// The build identity: the executable's length and modification time
    /// (docs/impl/stdlib-cache.md argues for both).
    pub(crate) fn identity(&self) -> (u64, u128) {
        (self.len, self.mtime)
    }

    /// The prefix every image this executable stores opens with, and the one
    /// a store prunes by.
    fn tag(&self) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        self.path.hash(&mut hasher);
        format!("{:016x}-", hasher.finish())
    }

    /// The file name this executable gives the image of the sources `digest`
    /// names.
    pub(crate) fn image_name(&self, digest: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        digest.hash(&mut hasher);
        self.identity().hash(&mut hasher);
        format!("{}{:016x}.image", self.tag(), hasher.finish())
    }
}

/// Store `cctx`'s boot state under `image`'s policy, and prune the images an
/// earlier build of this executable left behind.
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
    let Some(exe) = Executable::current() else {
        return;
    };
    let Some(path) = image.path_for(Some(&exe)) else {
        return;
    };
    let dir = path
        .parent()
        .expect("a cache path always names its directory");
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("[boot-image] mkdir failed: {e}");
        return;
    }
    match super::dump(heap, symbols, cctx, &path) {
        Ok(()) => prune_superseded(dir, &path, &exe.tag()),
        Err(e) => eprintln!("[boot-image] store failed: {e}"),
    }
}

/// Remove every image in `dir` that opens with `tag` except `keep` — the
/// megabytes each earlier build of this executable orphans. Another
/// executable's images stay, so binaries that share a directory do not evict
/// each other.
///
/// Call this after the store, never before: a store that fails must leave the
/// directory as it found it, still holding a file another process may be about
/// to map. A removal that fails is ignored; it is disk hygiene, and the next
/// store tries again.
fn prune_superseded(dir: &Path, keep: &Path, tag: &str) {
    let Ok(entries) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        let path = entry.path();
        let ours = path
            .file_name()
            .and_then(|n| n.to_str())
            .is_some_and(|n| n.starts_with(tag) && n.ends_with(".image"));
        if ours && path != keep {
            let _ = std::fs::remove_file(&path);
        }
    }
}
