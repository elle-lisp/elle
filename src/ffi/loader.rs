//! Dynamic library loading with platform abstraction.
//!
//! Elle library specs are written Linux-style (`libfoo.so`, `libfoo.so.2`).
//! On macOS and Windows the same spec is rewritten to the host's native form
//! (`libfoo.dylib`, `foo.dll`) by `library_candidates`, so a single
//! `(ffi/native "libfoo.so")` call is portable across platforms. Loading
//! itself requires a Unix-family `dlopen` (`#[cfg(unix)]`: Linux, macOS, BSD);
//! other platforms get error stubs.

/// Host OS family for dynamic-library naming.
///
/// Kept distinct from `cfg!(target_os = ...)` so the naming rules in
/// `library_candidates` can be unit-tested for every platform on any host.
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

/// Expand a Linux-style library spec into the ordered list of filenames to
/// try when loading on `os`.
///
/// Specs are written Linux-style. A bare `.so` or versioned `.so.N` soname is
/// rewritten to the host's native form so the same call works everywhere; a
/// directory prefix is preserved and only the basename is rewritten:
///
/// | spec               | macOS                            | Windows               |
/// |--------------------|----------------------------------|-----------------------|
/// | `libz.so`          | `libz.dylib`                     | `z.dll`, `libz.dll`   |
/// | `libcairo.so.2`    | `libcairo.2.dylib`, `libcairo.dylib` | `cairo.dll`, `libcairo.dll` |
///
/// Note the version moves: Linux appends it after `.so` (`libz.so.1`), macOS
/// embeds it before `.dylib` (`libz.1.dylib`). A spec that is not a Linux
/// soname (already `.dylib`/`.dll`, or an unrecognized name) is returned
/// unchanged — the caller is assumed to know the host form. The original spec
/// is always appended as a final fallback.
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

/// Handle to a loaded shared library.
pub(crate) struct LibraryHandle {
    /// Unique ID for this library in the FFI subsystem
    pub id: u32,
    /// Path to the library file actually loaded (a resolved candidate, which
    /// may differ from the requested spec — e.g. `libz.dylib` for `libz.so`)
    pub path: String,
    /// The underlying native library (Unix only)
    #[cfg(unix)]
    pub native: libloading::Library,
}

impl LibraryHandle {
    /// Get a raw pointer to a symbol in this library.
    ///
    /// # Arguments
    /// * `symbol_name` - The symbol to look up (e.g., "strlen")
    ///
    /// # Returns
    /// * `Ok(pointer)` - Raw function pointer
    /// * `Err(message)` - If symbol not found or other error
    pub fn get_symbol(&self, symbol_name: &str) -> Result<*const std::ffi::c_void, String> {
        #[cfg(unix)]
        {
            unsafe {
                self.native
                    .get::<*const std::ffi::c_void>(symbol_name.as_bytes())
                    .map(|sym| *sym)
                    .map_err(|e| {
                        format!("Symbol '{}' not found in {}: {}", symbol_name, self.path, e)
                    })
            }
        }

        #[cfg(not(unix))]
        {
            Err(format!(
                "Dynamic library loading not supported on this platform (attempted to load {})",
                self.path
            ))
        }
    }
}

/// Load a dynamic library from a Linux-style spec.
///
/// # Arguments
/// * `spec` - Library spec written Linux-style (`libfoo.so`, `libfoo.so.2`,
///   or an absolute/relative path). Rewritten to the host's native name(s)
///   via `library_candidates` and tried in order.
///
/// # Returns
/// * `Ok(library)` - Loaded library handle (its `path` is the candidate that
///   actually loaded)
/// * `Err(message)` - If no candidate could be loaded (file not found or not
///   a valid library)
///
/// # Example
/// ```text
/// Load a library and get a symbol:
/// let lib = load_library("/lib/x86_64-linux-gnu/libc.so.6")?;
/// let strlen_ptr = lib.get_symbol("strlen")?;
/// ```
pub(crate) fn load_library(spec: &str) -> Result<LibraryHandle, String> {
    #[cfg(unix)]
    {
        // Try the host-native name(s) for this spec in order, returning the
        // first that loads. This is what lets a Linux-style ".so" spec resolve
        // to ".dylib" on macOS without rewriting every call site.
        let candidates = library_candidates(spec, current_dl_os());
        let mut last_err = None;
        for cand in &candidates {
            // Only check existence for absolute/relative paths. Bare names
            // like "libm.so.6" are resolved by the dynamic linker via
            // LD_LIBRARY_PATH / /etc/ld.so.cache — don't reject them.
            if cand.contains('/') && !crate::path::exists(cand) {
                last_err = Some(format!("Library file not found: {}", cand));
                continue;
            }
            unsafe {
                match libloading::Library::new(cand) {
                    Ok(native) => {
                        return Ok(LibraryHandle {
                            id: 0, // Will be assigned by FFISubsystem
                            path: cand.clone(),
                            native,
                        });
                    }
                    Err(e) => last_err = Some(format!("Failed to load library '{}': {}", cand, e)),
                }
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

/// Load the current process as a library (equivalent to dlopen(NULL)).
///
/// This allows looking up symbols linked into the main executable,
/// including libc functions on most platforms.
///
/// # Returns
/// * `Ok(library)` - Handle to the current process
/// * `Err(message)` - If not supported on this platform
///
/// # Example
/// ```text
/// Load self and look up strlen:
/// let lib = load_self()?;
/// let strlen_ptr = lib.get_symbol("strlen")?;
/// ```
pub(crate) fn load_self() -> Result<LibraryHandle, String> {
    #[cfg(unix)]
    {
        use libloading::os::unix::Library as UnixLibrary;
        let unix_lib = UnixLibrary::this();
        Ok(LibraryHandle {
            id: 0, // Will be assigned by FFISubsystem
            path: "<self>".to_string(),
            native: unix_lib.into(),
        })
    }

    #[cfg(not(unix))]
    {
        Err("Self-process loading not supported on this platform".to_string())
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

    #[test]
    #[cfg(unix)]
    fn test_load_libc() {
        // Load system libc
        let lib = load_library("/lib/x86_64-linux-gnu/libc.so.6")
            .or_else(|_| load_library("/lib64/libc.so.6"))
            .or_else(|_| load_library("libc.so.6"));

        // If libc is findable, test loading succeeds
        if let Ok(lib) = lib {
            assert!(!lib.path.is_empty());
        }
    }

    #[test]
    fn test_missing_file() {
        let result = load_library("/nonexistent/library.so");
        assert!(result.is_err());
    }

    #[test]
    #[cfg(unix)]
    fn test_get_symbol_strlen() {
        let lib = load_library("/lib/x86_64-linux-gnu/libc.so.6")
            .or_else(|_| load_library("/lib64/libc.so.6"))
            .or_else(|_| load_library("libc.so.6"));

        if let Ok(lib) = lib {
            let result = lib.get_symbol("strlen");
            // strlen should exist in libc
            if let Ok(sym) = result {
                assert!(!sym.is_null());
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_get_symbol_missing() {
        let lib = load_library("/lib/x86_64-linux-gnu/libc.so.6")
            .or_else(|_| load_library("/lib64/libc.so.6"))
            .or_else(|_| load_library("libc.so.6"));

        if let Ok(lib) = lib {
            let result = lib.get_symbol("this_function_does_not_exist_in_libc_12345");
            assert!(result.is_err());
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_load_self() {
        let lib = load_self();
        assert!(lib.is_ok());
        let lib = lib.unwrap();
        assert_eq!(lib.path, "<self>");
        // Should be able to find libc symbols
        let result = lib.get_symbol("strlen");
        assert!(result.is_ok());
        assert!(!result.unwrap().is_null());
    }
}
