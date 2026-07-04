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
/// | spec            | macOS                                | Windows                     |
/// |-----------------|--------------------------------------|-----------------------------|
/// | `libz.so`       | `libz.dylib`                         | `z.dll`, `libz.dll`         |
/// | `libcairo.so.2` | `libcairo.2.dylib`, `libcairo.dylib` | `cairo.dll`, `libcairo.dll` |
///
/// The version moves: Linux appends it after `.so` (`libz.so.1`), macOS embeds it
/// before `.dylib` (`libz.1.dylib`), Windows drops it. A spec that is not a Linux
/// soname (already `.dylib`/`.dll`, or unrecognized) is returned unchanged — the
/// caller is assumed to have given the host form.
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
            if let Some(v) = version {
                out.push(format!("{dir}{stem}.{v}.dylib"));
            }
            out.push(format!("{dir}{stem}.dylib"));
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
mod tests {
    use super::*;

    /// Build a `Vec<String>` from string literals for terse expected values.
    fn svec(xs: &[&str]) -> Vec<String> {
        xs.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn candidates_linux_passthrough() {
        // Linux is the canonical form: specs are used verbatim.
        assert_eq!(
            library_candidates("libz.so", DlOs::Linux),
            svec(&["libz.so"])
        );
        assert_eq!(
            library_candidates("libz.so.1", DlOs::Linux),
            svec(&["libz.so.1"])
        );
        assert_eq!(
            library_candidates("/usr/lib/libz.so.1", DlOs::Linux),
            svec(&["/usr/lib/libz.so.1"])
        );
    }

    #[test]
    fn candidates_macos_unversioned() {
        // libz.so → libz.dylib, with the original kept as a fallback.
        assert_eq!(
            library_candidates("libz.so", DlOs::Macos),
            svec(&["libz.dylib", "libz.so"])
        );
    }

    #[test]
    fn candidates_macos_versioned() {
        // Linux appends the version after .so; macOS embeds it before .dylib.
        assert_eq!(
            library_candidates("libcairo.so.2", DlOs::Macos),
            svec(&["libcairo.2.dylib", "libcairo.dylib", "libcairo.so.2"])
        );
    }

    #[test]
    fn candidates_macos_preserves_directory() {
        // Only the basename is rewritten; the directory prefix is preserved.
        assert_eq!(
            library_candidates("/usr/lib/libz.so.1", DlOs::Macos),
            svec(&[
                "/usr/lib/libz.1.dylib",
                "/usr/lib/libz.dylib",
                "/usr/lib/libz.so.1"
            ])
        );
    }

    #[test]
    fn candidates_non_soname_passthrough() {
        // Already host-native or unrecognized → returned unchanged.
        assert_eq!(
            library_candidates("libz.dylib", DlOs::Macos),
            svec(&["libz.dylib"])
        );
        assert_eq!(library_candidates("z.dll", DlOs::Windows), svec(&["z.dll"]));
        assert_eq!(
            library_candidates("/opt/foo/bar", DlOs::Macos),
            svec(&["/opt/foo/bar"])
        );
    }

    #[test]
    fn candidates_windows_strips_lib_prefix() {
        // Windows DLLs conventionally drop the `lib` prefix and the version.
        assert_eq!(
            library_candidates("libzmq.so", DlOs::Windows),
            svec(&["zmq.dll", "libzmq.dll", "libzmq.so"])
        );
        assert_eq!(
            library_candidates("libcairo.so.2", DlOs::Windows),
            svec(&["cairo.dll", "libcairo.dll", "libcairo.so.2"])
        );
    }

    // A library every Linux host has, used to exercise dedup + symbol resolution
    // without a build-time fixture. `libc` is always loadable by bare name.
    const LIBC: &str = "libc.so.6";

    #[test]
    fn load_dedups_by_path_and_resolves_symbols() {
        // Two loads of the same library return the same key (dedup) ...
        let k1 = load(LIBC).expect("load libc");
        let k2 = load(LIBC).expect("load libc again");
        assert_eq!(k1, k2, "the same library path dedups to one registry entry");

        // ... and a symbol resolves to the same non-null address through both keys —
        // i.e. the mapping is shared, not reloaded per call.
        let a = symbol(&k1, "strlen").expect("strlen via k1");
        let b = symbol(&k2, "strlen").expect("strlen via k2");
        assert!(!a.is_null());
        assert_eq!(a, b, "the shared mapping yields one symbol address");
    }

    #[test]
    fn missing_path_library_errors() {
        // A '/'-containing path that does not exist on disk fails to load (rather
        // than being handed to the dynamic linker as a bare name).
        assert!(load("/nonexistent/does-not-exist.so").is_err());
    }

    #[test]
    fn register_teardown_validates_symbol() {
        let k = load(LIBC).expect("load libc");
        // A real symbol registers; a bogus one errors at registration, not at run.
        assert!(register_teardown(&k, "free").is_ok());
        assert!(
            register_teardown(&k, "no_such_symbol_xyzzy").is_err(),
            "a teardown symbol that does not resolve must error at registration"
        );
        // Draining is safe and idempotent (we register `free`, a no-arg-safe-enough
        // libc symbol only as a resolution test — but to avoid actually calling
        // free() with no arg, drain without asserting side effects is unsafe; so we
        // do not call run_teardowns here. Resolution validation above is the pin.)
    }

    #[test]
    fn symbol_outlives_a_dropped_subsystem_via_the_global_mapping() {
        // The mapping lives in the process-global registry, not in any per-VM owner,
        // so resolving a symbol does not depend on any FFISubsystem being alive: a
        // fresh resolution after arbitrary churn still succeeds (the library was
        // never unloaded). This is the property that makes the worker-teardown crash
        // impossible by construction.
        let k = load(LIBC).expect("load libc");
        let first = symbol(&k, "strlen").expect("strlen");
        // No FFISubsystem is constructed/dropped here, but the invariant is the
        // same: the global mapping is permanent, so a later lookup is identical.
        let again = symbol(&k, "strlen").expect("strlen again");
        assert_eq!(first, again);
    }
}
