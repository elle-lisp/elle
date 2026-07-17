//! Process-global FFI library registry — the owner that makes a loaded library's
//! mapping outlive every thread.
//!
//! ## Why this exists
//!
//! A loaded shared library registers per-thread cleanup with the C runtime: a
//! library like libgit2 (via OpenSSL) calls `pthread_key_create(&key, destructor)`
//! and stores a non-null value under that key on each thread that uses it, so glibc
//! invokes `destructor` from `__nptl_deallocate_tsd` as the OS thread exits. If the
//! library is `dlclose`d before that thread exits — which happens when an
//! `os/spawn` worker owns the only handle and drops it on teardown — the
//! thread-exit walk jumps into the now-unmapped page and the whole process dies
//! with SIGSEGV.
//!
//! The fix is the same discipline the plugin loader already uses
//! (`src/plugin.rs`: `std::mem::forget(lib)` — "plugins are never unloaded"):
//! **own the mapping process-globally and never `dlclose` it.** `dlopen`/`dlclose`
//! is refcounted process-wide, so one permanent holder keeps the refcount ≥ 1; a
//! worker that loads and exits never drives it to 0, so its later TSD destructor
//! always lands in mapped code. This is robust *independent of the exit path*
//! (`std::process::exit`/`_exit`/signals all leave the mapping in place — the OS
//! reclaims it at process death).
//!
//! ## Teardown is explicit, never automatic
//!
//! A program may attach an **ordered teardown destructor** to a library
//! (`register_teardown`, surfaced as `ffi/on-unload`) — e.g. libgit2's
//! `git_libgit2_shutdown`. These run only when the program explicitly asks
//! (`run_teardowns`, surfaced as `ffi/run-teardowns`), in reverse load order.
//! The runtime never runs them on its own: a teardown like `git_libgit2_shutdown`
//! deletes the pthread key it created, which races a *detached* worker still in its
//! own TSD-destructor walk — so the decision that all such workers have quiesced
//! (e.g. via `sys/join`) is the programmer's, made explicitly, not the runtime's
//! made behind their back. Teardown **never `dlclose`s**: the mapping stays for the
//! process lifetime regardless, which is exactly what keeps a late worker's TSD
//! walk safe. Skipping teardown entirely is therefore always safe.

use std::collections::HashMap;
use std::ffi::c_void;
use std::path::PathBuf;
use std::sync::{Mutex, OnceLock};

/// One process-global entry per canonical library path. Owns the `libloading::Library`
/// (the mapping) for the process lifetime — it is never dropped, so `dlclose` never
/// runs (see module docs). `teardowns` are C symbols in this library to call, in
/// order, when the program explicitly tears down.
struct LoadedLib {
    native: libloading::Library,
    teardowns: Vec<String>,
}

/// The registry: load order (teardown runs in reverse) plus a path→index map for
/// O(1) dedup.
struct Registry {
    order: Vec<(PathBuf, LoadedLib)>,
    by_path: HashMap<PathBuf, usize>,
}

/// The process-global registry. `libloading::Library` is `Send + Sync` on unix, so
/// holding libraries here and resolving their symbols from worker threads (under the
/// mutex) is sound.
static REGISTRY: OnceLock<Mutex<Registry>> = OnceLock::new();

fn registry() -> &'static Mutex<Registry> {
    REGISTRY.get_or_init(|| {
        Mutex::new(Registry {
            order: Vec::new(),
            by_path: HashMap::new(),
        })
    })
}

/// The dedup key for a library path. On-disk paths canonicalize so two spellings of
/// the same file share one entry; a dynamic-linker-resolved bare name (`libm.so.6`)
/// or the self-sentinel doesn't canonicalize and is keyed by its raw string. Worst
/// case for a non-canonicalizable name is a second `dlopen` of the same library
/// (an extra refcount, never a crash — nothing is ever unmapped).
fn canon_key(path: &str) -> PathBuf {
    std::fs::canonicalize(path).unwrap_or_else(|_| PathBuf::from(path))
}

/// Host OS family for dynamic-library naming. Kept distinct from
/// `cfg!(target_os = ...)` so the naming rules in [`library_candidates`] can be
/// unit-tested for every platform on any host.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub(crate) enum DlOs {
    Linux,
    Macos,
    Windows,
}

/// The OS family this binary was compiled for.
pub(crate) fn current_dl_os() -> DlOs {
    if cfg!(target_os = "macos") {
        DlOs::Macos
    } else if cfg!(target_os = "windows") {
        DlOs::Windows
    } else {
        // Linux, BSD, and other dlopen-capable Unixes use the soname scheme.
        DlOs::Linux
    }
}

/// Expand a Linux-style library spec into the ordered filenames to try when
/// loading on `os`.
///
/// Elle specs are written Linux-style (`libfoo.so`, `libfoo.so.N`), so a single
/// `(ffi/native "libfoo.so")` call is portable: on macOS/Windows the basename is
/// rewritten to the host's native form (a directory prefix is preserved), and the
/// original spec is always appended as a final fallback.
///
/// | spec            | macOS                                                        | Windows                     |
/// |-----------------|--------------------------------------------------------------|-----------------------------|
/// | `libz.so`       | `libz.dylib`, `…/homebrew/lib/libz.dylib`, `…/local/…`       | `z.dll`, `libz.dll`         |
/// | `libcairo.so.2` | `libcairo.2.dylib` (+ prefixes), `libcairo.dylib` (+ prefixes)| `cairo.dll`, `libcairo.dll` |
///
/// The version moves: Linux appends it after `.so` (`libz.so.1`), macOS embeds it
/// before `.dylib` (`libz.1.dylib`), Windows drops it. On macOS a BARE soname also
/// probes the standard Homebrew prefixes (`/opt/homebrew/lib`, `/usr/local/lib`),
/// since dyld does not search `/opt/homebrew/lib` for a bare name and that is where
/// non-system libraries (libzstd, libcairo, …) live; a spec that already names a
/// directory is honored verbatim. A spec that is not a Linux soname (already
/// `.dylib`/`.dll`, or unrecognized) is returned unchanged — the caller is assumed
/// to have given the host form.
pub(crate) fn library_candidates(spec: &str, os: DlOs) -> Vec<String> {
    if os == DlOs::Linux {
        return vec![spec.to_string()];
    }

    // Split off a directory prefix; only the basename carries the soname.
    let (dir, base) = match spec.rfind('/') {
        Some(i) => (&spec[..=i], &spec[i + 1..]),
        None => ("", spec),
    };

    // Parse a Linux soname: "libfoo.so" or "libfoo.so.<version>".
    const SO_VER: &str = ".so.";
    let (stem, version) = if let Some(stem) = base.strip_suffix(".so") {
        (stem, None)
    } else if let Some(idx) = base.find(SO_VER) {
        (&base[..idx], Some(&base[idx + SO_VER.len()..]))
    } else {
        // Not a Linux soname — assume the caller already gave the host form.
        return vec![spec.to_string()];
    };

    let mut out = Vec::new();
    match os {
        DlOs::Macos => {
            // macOS embeds the version before the extension: libfoo.2.dylib.
            // Emit the versioned basename first, then the unversioned one; each
            // native basename also probes the standard Homebrew prefixes when the
            // spec is a BARE name. dyld searches /usr/lib and the shared cache for
            // a bare name but NOT /opt/homebrew/lib (Apple Silicon), so a non-system
            // library installed by Homebrew (libzstd, libcairo, …) is otherwise
            // unreachable via `(ffi/native "libfoo.so")`. /usr/local/lib (Intel
            // Homebrew) is already in dyld's fallback path, but we probe it too so
            // the on-disk existence check finds it regardless of the linker's mood.
            // A spec with an explicit directory named a path — honor it verbatim.
            let mut bases = Vec::new();
            if let Some(v) = version {
                bases.push(format!("{stem}.{v}.dylib"));
            }
            bases.push(format!("{stem}.dylib"));
            for base in &bases {
                out.push(format!("{dir}{base}"));
                if dir.is_empty() {
                    out.push(format!("/opt/homebrew/lib/{base}"));
                    out.push(format!("/usr/local/lib/{base}"));
                }
            }
        }
        DlOs::Windows => {
            // Windows DLLs drop the `lib` prefix and carry no soname version.
            let win = stem.strip_prefix("lib").unwrap_or(stem);
            out.push(format!("{dir}{win}.dll"));
            out.push(format!("{dir}{stem}.dll"));
        }
        DlOs::Linux => unreachable!("Linux handled above"),
    }
    // Keep the original spec as a final fallback.
    out.push(spec.to_string());
    out
}

/// Load (or reuse) a shared library, returning its registry key. The mapping is
/// held for the process lifetime — never `dlclose`d.
///
/// `spec` is written Linux-style (`libfoo.so`, `libfoo.so.2`, or an
/// absolute/relative path); [`library_candidates`] rewrites it to the host's
/// native name(s) and each is tried in order, so the same call is portable across
/// Linux/macOS/Windows. A candidate containing `/` must exist on disk; a bare name
/// is left to the dynamic linker (`LD_LIBRARY_PATH` / `ld.so.cache`). Dedup keys on
/// the candidate that actually loaded, so two spellings of one file share an entry.
pub(crate) fn load(spec: &str) -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let candidates = library_candidates(spec, current_dl_os());
        let mut reg = registry().lock().expect("ffi registry mutex poisoned");
        let mut last_err = None;
        for cand in &candidates {
            // Only check existence for path-form candidates; a bare name is
            // resolved by the dynamic linker, so don't reject it here.
            if cand.contains('/') && !crate::path::exists(cand) {
                last_err = Some(format!("Library file not found: {}", cand));
                continue;
            }
            let key = canon_key(cand);
            if reg.by_path.contains_key(&key) {
                return Ok(key);
            }
            match unsafe { libloading::Library::new(cand) } {
                Ok(native) => {
                    let idx = reg.order.len();
                    reg.order.push((
                        key.clone(),
                        LoadedLib {
                            native,
                            teardowns: Vec::new(),
                        },
                    ));
                    reg.by_path.insert(key.clone(), idx);
                    return Ok(key);
                }
                Err(e) => last_err = Some(format!("Failed to load library '{}': {}", cand, e)),
            }
        }
        Err(last_err.unwrap_or_else(|| format!("Failed to load library '{}'", spec)))
    }
    #[cfg(not(unix))]
    {
        let _ = current_dl_os(); // keep the helper exercised on all targets
        Err(format!(
            "Dynamic library loading only supported on Unix (attempted to load {})",
            spec
        ))
    }
}

/// Load the current process as a library (`dlopen(NULL)`), returning its registry
/// key (the `<self>` sentinel). Like [`load`], the mapping is process-lifetime.
pub(crate) fn load_self() -> Result<PathBuf, String> {
    #[cfg(unix)]
    {
        let key = PathBuf::from("<self>");
        let mut reg = registry().lock().expect("ffi registry mutex poisoned");
        if reg.by_path.contains_key(&key) {
            return Ok(key);
        }
        let native: libloading::Library = {
            use libloading::os::unix::Library as UnixLibrary;
            UnixLibrary::this().into()
        };
        let idx = reg.order.len();
        reg.order.push((
            key.clone(),
            LoadedLib {
                native,
                teardowns: Vec::new(),
            },
        ));
        reg.by_path.insert(key.clone(), idx);
        Ok(key)
    }
    #[cfg(not(unix))]
    {
        Err("Self-process loading not supported on this platform".to_string())
    }
}

/// Resolve a symbol in the library named by `key`, returning a raw pointer. The
/// pointer is valid for the process lifetime — the mapping is never unloaded — so it
/// stays usable after the registry lock is released (e.g. by a later `ffi/call`).
pub(crate) fn symbol(key: &PathBuf, sym: &str) -> Result<*const c_void, String> {
    let reg = registry().lock().expect("ffi registry mutex poisoned");
    let &idx = reg
        .by_path
        .get(key)
        .ok_or_else(|| format!("library '{}' not loaded", key.display()))?;
    let lib = &reg.order[idx].1.native;
    unsafe {
        lib.get::<*const c_void>(sym.as_bytes())
            .map(|s| *s)
            .map_err(|e| format!("Symbol '{}' not found in {}: {}", sym, key.display(), e))
    }
}

/// Register an ordered teardown for the library named by `key`: a zero-arg C symbol
/// (e.g. `"git_libgit2_shutdown"`) to call when the program explicitly tears down.
/// The symbol is resolved now so a typo errors immediately rather than silently at
/// teardown.
pub(crate) fn register_teardown(key: &PathBuf, sym: &str) -> Result<(), String> {
    let mut reg = registry().lock().expect("ffi registry mutex poisoned");
    let &idx = reg
        .by_path
        .get(key)
        .ok_or_else(|| format!("library '{}' not loaded", key.display()))?;
    // Validate the symbol exists in this library before recording it.
    unsafe {
        reg.order[idx]
            .1
            .native
            .get::<unsafe extern "C" fn()>(sym.as_bytes())
            .map_err(|e| {
                format!(
                    "teardown symbol '{}' not found in {}: {}",
                    sym,
                    key.display(),
                    e
                )
            })?;
    }
    reg.order[idx].1.teardowns.push(sym.to_string());
    Ok(())
}

/// Run every registered teardown, in **reverse load order**, draining them (a
/// second call is a no-op). Each is a zero-arg C function in its still-mapped
/// library; this **never `dlclose`s** — the mapping stays for the process lifetime.
/// Explicit-only: the runtime never calls this itself (see module docs).
pub(crate) fn run_teardowns() {
    // Collect the function pointers under the lock, then call them after releasing
    // it: a teardown is foreign C that must not be run holding the registry mutex
    // (it could, in principle, re-enter and deadlock a std `Mutex`). The pointers
    // are into permanently-mapped libraries, so they stay valid after unlock.
    let mut fns: Vec<unsafe extern "C" fn()> = Vec::new();
    {
        let mut reg = registry().lock().expect("ffi registry mutex poisoned");
        // Reverse load order: tear down dependents before dependencies.
        for i in (0..reg.order.len()).rev() {
            let syms: Vec<String> = std::mem::take(&mut reg.order[i].1.teardowns);
            for sym in syms {
                if let Ok(f) = unsafe {
                    reg.order[i]
                        .1
                        .native
                        .get::<unsafe extern "C" fn()>(sym.as_bytes())
                } {
                    fns.push(*f);
                }
            }
        }
    }
    for f in fns {
        unsafe { f() };
    }
}

#[cfg(test)]
mod tests;
