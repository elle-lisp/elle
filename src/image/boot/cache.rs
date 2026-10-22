// audited: 2026-10-21
//! The boot-image policy and the warm cache: which images an instance reads,
//! where a compiled boot stores one, and what a store leaves behind.
//!
//! docs/impl/image/boot.md
//!
//! The file is named for the source digest and the binary's identity, so an
//! edit to a boot source misses and a rebuild never opens the last build's
//! image. A store writes a temporary file and renames it over the
//! target — the dumper's own discipline, because a live hydration holds a
//! private mapping of the old inode.

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

use crate::image::{ImageError, ImageSource};
use crate::symbol::SymbolTable;
use crate::value::fiberheap::FiberHeap;

use super::{sources_digest, Blob, BootImage, OpenImage};

impl BootImage {
    /// The blob this policy hydrates before it reads the warm cache, or `None`
    /// when it reads none: `Process` reads the blob this binary carries
    /// (docs/impl/image/boot.md).
    pub(crate) fn blob(&self) -> Option<Blob> {
        match self {
            BootImage::Process => Blob::this_binary(),
            BootImage::Embedded { blob, .. } => Some(*blob),
            BootImage::Off | BootImage::Dir(_) => None,
        }
    }

    /// The warm-cache directory this policy reads and writes, or `None` when
    /// it reads none.
    fn dir(&self) -> Option<PathBuf> {
        self.dir_in(crate::config::get())
    }

    /// The warm-cache directory this policy reads and writes under `config`,
    /// or `None` when it reads none.
    pub(crate) fn dir_in(&self, config: &crate::config::Config) -> Option<PathBuf> {
        match self {
            BootImage::Off => None,
            BootImage::Embedded { fallback, .. } => fallback.clone(),
            BootImage::Dir(d) => Some(d.clone()),
            BootImage::Process => config
                .cache
                .as_ref()
                .map(|base| PathBuf::from(base).join("boot")),
        }
    }

    /// The warm-cache file this binary reads and writes under this policy, or
    /// `None` when the policy reads no warm cache or the binary cannot
    /// identify itself.
    pub fn path(&self) -> Option<PathBuf> {
        self.path_for(Executable::current().as_ref())
    }

    /// The image file `exe` reads and writes under this policy. An executable
    /// that could not be measured gets none, rather than a name another build
    /// could share.
    pub(crate) fn path_for(&self, exe: Option<&Executable>) -> Option<PathBuf> {
        Some(self.dir()?.join(exe?.image_name(&sources_digest())))
    }

    /// The images this policy reads, held open, in the order a boot tries them:
    /// the blob, then the warm-cache file (docs/impl/image/boot.md).
    pub(crate) fn images(&self) -> Vec<OpenImage> {
        let blob = self.blob().and_then(|blob| match blob.open() {
            Ok(open) => Some(open),
            Err(e) => {
                eprintln!("[boot-image] the blob does not open: {e}");
                None
            }
        });
        blob.into_iter().chain(self.open()).collect()
    }

    /// The warm-cache file this policy names, held open, or `None` when there
    /// is no file to read. A file that exists and does not open is reported,
    /// for the reason a refused hydration is.
    pub(crate) fn open(&self) -> Option<OpenImage> {
        let path = self.path()?;
        match ImageSource::open(&path) {
            Ok(source) => Some(OpenImage::file(source, path)),
            Err(ImageError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("[boot-image] {}: {e}", path.display());
                None
            }
        }
    }
}

/// The running executable, as the warm cache keys its image: by the build
/// identity, which the stdlib cache keys on too.
pub(crate) struct Executable {
    len: u64,
    mtime: u128,
}

impl Executable {
    pub(crate) fn new(len: u64, mtime: u128) -> Self {
        Executable { len, mtime }
    }

    /// The running executable, or `None` when it cannot be located or
    /// measured.
    pub(crate) fn current() -> Option<Self> {
        let path = std::env::current_exe().ok()?;
        let meta = std::fs::metadata(path).ok()?;
        let mtime = meta
            .modified()
            .ok()?
            .duration_since(std::time::UNIX_EPOCH)
            .ok()?
            .as_nanos();
        Some(Executable::new(meta.len(), mtime))
    }

    /// The build identity: the executable's length and modification time
    /// (docs/impl/stdlib-cache.md argues for both).
    pub(crate) fn identity(&self) -> (u64, u128) {
        (self.len, self.mtime)
    }

    /// The file name this executable gives the image of the sources `digest`
    /// names.
    pub(crate) fn image_name(&self, digest: &str) -> String {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        digest.hash(&mut hasher);
        self.identity().hash(&mut hasher);
        format!("{:016x}.image", hasher.finish())
    }
}

/// Store `cctx`'s boot state under `image`'s policy, prune every other image
/// in its directory, and answer the stored image, held open.
///
/// The answer is the file this store wrote, whatever renames over its path
/// later, which is what lets the storing instance hand it to its workers
/// (docs/impl/image/boot.md).
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
) -> Option<OpenImage> {
    let path = image.path()?;
    let dir = path
        .parent()
        .expect("a cache path always names its directory");
    if let Err(e) = std::fs::create_dir_all(dir) {
        eprintln!("[boot-image] mkdir failed: {e}");
        return None;
    }
    let stored = super::dump(heap, symbols, cctx, &path)
        .and_then(|file| ImageSource::at(file, 0))
        .map(|source| OpenImage::file(source, path.clone()));
    match stored {
        Ok(open) => {
            prune_superseded(dir, &path);
            Some(open)
        }
        Err(e) => {
            eprintln!("[boot-image] store failed: {e}");
            None
        }
    }
}

/// Remove every image in `dir` except `keep`, another executable's included.
/// That bounds the directory at one image, however many builds and test
/// binaries boot in it (docs/impl/image/boot.md).
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

#[cfg(test)]
mod tests;
