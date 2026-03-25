//! Dynamic library loading with platform abstraction.
//!
//! Supports loading shared libraries on Unix platforms (.so on Linux, .dylib on macOS).

/// Handle to a loaded shared library.
pub(crate) struct LibraryHandle {
    /// Unique ID for this library in the FFI subsystem
    pub id: u32,
    /// Path to the library file
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

/// Load a dynamic library from a file path.
///
/// # Arguments
/// * `path` - Path to the library file (.so on Linux)
///
/// # Returns
/// * `Ok(library)` - Loaded library handle
/// * `Err(message)` - If file not found or not a valid library
///
/// # Example
/// ```text
/// Load a library and get a symbol:
/// let lib = load_library("/lib/x86_64-linux-gnu/libc.so.6")?;
/// let strlen_ptr = lib.get_symbol("strlen")?;
/// ```
pub(crate) fn load_library(path: &str) -> Result<LibraryHandle, String> {
    #[cfg(unix)]
    {
        // Only check existence for absolute/relative paths.
        // Bare names like "libm.so.6" / "libSystem.B.dylib" are resolved
        // by the dynamic linker (LD_LIBRARY_PATH / DYLD_LIBRARY_PATH /
        // /etc/ld.so.cache) — don't reject them.
        if path.contains('/') && !crate::path::exists(path) {
            return Err(format!("Library file not found: {}", path));
        }

        unsafe {
            match libloading::Library::new(path) {
                Ok(native) => Ok(LibraryHandle {
                    id: 0, // Will be assigned by FFISubsystem
                    path: path.to_string(),
                    native,
                }),
                Err(e) => Err(format!("Failed to load library '{}': {}", path, e)),
            }
        }
    }

    #[cfg(not(unix))]
    {
        Err(format!(
            "Dynamic library loading only supported on Unix (attempted to load {})",
            path
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

    /// Try to load libc by platform-appropriate paths.
    #[cfg(unix)]
    fn try_load_libc() -> Result<LibraryHandle, String> {
        // Linux paths
        load_library("/lib/x86_64-linux-gnu/libc.so.6")
            .or_else(|_| load_library("/lib64/libc.so.6"))
            .or_else(|_| load_library("libc.so.6"))
            // macOS: libSystem includes libc
            .or_else(|_| load_library("libSystem.B.dylib"))
    }

    #[test]
    #[cfg(unix)]
    fn test_load_libc() {
        if let Ok(lib) = try_load_libc() {
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
        if let Ok(lib) = try_load_libc() {
            let result = lib.get_symbol("strlen");
            if let Ok(sym) = result {
                assert!(!sym.is_null());
            }
        }
    }

    #[test]
    #[cfg(unix)]
    fn test_get_symbol_missing() {
        if let Ok(lib) = try_load_libc() {
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
