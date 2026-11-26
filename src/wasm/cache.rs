// audited: 2026-11-25
//! The WASM caches beneath `--cache=`: one build's directory, the compiled modules in it, and wasmtime's compile cache.
//!
//! docs/impl/wasm.md

use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use wasmtime::{Engine, Module, Result};

use crate::image::boot::Executable;

/// This build's directory beneath the `--cache=` directory, opened once per
/// process, or `None` when caching is off or the binary cannot identify
/// itself.
pub(crate) fn build_dir() -> Option<&'static Path> {
    static DIR: OnceLock<Option<PathBuf>> = OnceLock::new();
    DIR.get_or_init(|| {
        let root = crate::config::get().cache.as_ref()?;
        let exe = Executable::current()?;
        open_build_dir(Path::new(root), &build_name(&exe))
    })
    .as_deref()
}

/// The directory name of the build `exe` is: a hash of its identity.
pub(crate) fn build_name(exe: &Executable) -> String {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    exe.identity().hash(&mut hasher);
    format!("{:016x}", hasher.finish())
}

/// Open `root/wasm/BUILD`: create it, and remove every other entry beside it,
/// so the caches hold one build's entries (docs/impl/wasm.md). A removal that
/// fails is ignored; the next open tries again.
pub(crate) fn open_build_dir(root: &Path, build: &str) -> Option<PathBuf> {
    let builds = root.join("wasm");
    let dir = builds.join(build);
    std::fs::create_dir_all(&dir).ok()?;
    for entry in std::fs::read_dir(&builds).into_iter().flatten().flatten() {
        let path = entry.path();
        if path == dir {
            continue;
        }
        let _ = if path.is_dir() {
            std::fs::remove_dir_all(&path)
        } else {
            std::fs::remove_file(&path)
        };
    }
    Some(dir)
}

/// Cache the compiles of the engine `config` builds in `dir`.
pub(crate) fn cache_compiles_in(config: &mut wasmtime::Config, dir: &Path) {
    std::fs::create_dir_all(dir).ok();
    config
        .enable_incremental_compilation(std::sync::Arc::new(DiskCache(dir.to_path_buf())))
        .ok();
}

/// Disk-backed compilation cache for wasmtime incremental compilation.
/// Cache entries are stored as files named by hex-encoded key hash.
#[derive(Debug)]
struct DiskCache(PathBuf);

impl wasmtime::CacheStore for DiskCache {
    fn get(&self, key: &[u8]) -> Option<std::borrow::Cow<'_, [u8]>> {
        let path = self.0.join(hex_name(key));
        std::fs::read(&path).ok().map(std::borrow::Cow::Owned)
    }

    fn insert(&self, key: &[u8], value: Vec<u8>) -> bool {
        let path = self.0.join(hex_name(key));
        atomic_write(&path, &value)
    }
}

/// Where this build keeps the compiled artifact for `wasm_bytes`, or `None`
/// when it caches nothing. `kind` separates the two module shapes that share
/// the directory ("closure", "module").
pub(crate) fn cache_path_for(kind: &str, wasm_bytes: &[u8]) -> Option<PathBuf> {
    let mut hasher = std::collections::hash_map::DefaultHasher::new();
    wasm_bytes.hash(&mut hasher);
    Some(build_dir()?.join(format!("{kind}_{:016x}.bin", hasher.finish())))
}

/// Compile `wasm_bytes`, reusing the artifact at `cache_path` when one is
/// there and this wasmtime can load it.
///
/// A cache read that does not yield a usable module is a MISS: the name records
/// a hash of the WASM bytes and nothing else, while `Module::deserialize`
/// accepts an artifact only from the wasmtime that wrote it. Compiling fresh
/// and overwriting repairs the entry in place, which is what keeps a wasmtime
/// upgrade from stranding every warm cache (docs/impl/wasm.md).
pub(crate) fn cached_or_compile(
    engine: &Engine,
    wasm_bytes: &[u8],
    cache_path: Option<&Path>,
) -> Result<Module> {
    let Some(path) = cache_path else {
        return Module::new(engine, wasm_bytes);
    };

    if let Ok(bytes) = std::fs::read(path) {
        // SAFETY: the cache directory holds artifacts this process wrote.
        if let Ok(module) = unsafe { Module::deserialize(engine, &bytes) } {
            return Ok(module);
        }
    }

    let module = Module::new(engine, wasm_bytes)?;
    if let Ok(serialized) = module.serialize() {
        atomic_write(path, &serialized);
    }
    Ok(module)
}

/// Write data to a file atomically: write to a temp file in the same
/// directory, then rename into place. Prevents readers from seeing
/// partial writes. A directory another build removed fails the write.
fn atomic_write(path: &Path, data: &[u8]) -> bool {
    use std::io::Write;
    let Some(dir) = path.parent() else {
        return false;
    };
    let Ok(mut tmp) = tempfile::NamedTempFile::new_in(dir) else {
        return false;
    };
    if tmp.write_all(data).is_err() {
        return false;
    }
    tmp.persist(path).is_ok()
}

fn hex_name(key: &[u8]) -> String {
    use std::fmt::Write;
    let mut s = String::with_capacity(key.len() * 2);
    for b in key {
        write!(s, "{:02x}", b).ok();
    }
    s
}
