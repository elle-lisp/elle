// audited: 2026-10-20
//! The warm cache: where a boot image is read from, where a compiled boot
//! stores one, and what a store leaves behind.
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

use super::{sources_digest, BootImage};

/// A boot image held open: the descriptor a hydration maps, and the path it was
/// opened at, which a refusal names.
///
/// A worker hydrates from its parent's, so the descriptor travels rather than
/// the path (docs/impl/image/boot.md). A prune or a rename over the path after
/// this opened it changes nothing a hydration maps.
#[derive(Debug)]
pub(crate) struct OpenImage {
    source: ImageSource,
    origin: PathBuf,
}

impl OpenImage {
    /// The descriptor and offset a hydration maps.
    pub(crate) fn source(&self) -> &ImageSource {
        &self.source
    }

    /// The path the image was opened at.
    pub(crate) fn origin(&self) -> &Path {
        &self.origin
    }
}

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

    /// The image file this policy names, held open, or `None` when there is no
    /// file to read. A file that exists and does not open is reported, for the
    /// reason a refused hydration is.
    pub(crate) fn open(&self) -> Option<OpenImage> {
        let origin = self.path()?;
        match ImageSource::open(&origin) {
            Ok(source) => Some(OpenImage { source, origin }),
            Err(ImageError::Io(e)) if e.kind() == std::io::ErrorKind::NotFound => None,
            Err(e) => {
                eprintln!("[boot-image] {}: {e}", origin.display());
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
        .map(|source| OpenImage {
            source,
            origin: path.clone(),
        });
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
