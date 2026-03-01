//! UTF-8 path operations.
//!
//! Single abstraction over camino, path-clean, and pathdiff.
//! No other module in the crate imports these crates directly.
//! Public API is `&str` → `String` / `&str` / `bool` / `Result`.

use camino::{Utf8Path, Utf8PathBuf};

// =============================================================================
// Pure operations (no filesystem access)
// =============================================================================

/// Join path components. Absolute components replace the prefix.
pub fn join(components: &[&str]) -> String {
    let mut buf = Utf8PathBuf::new();
    for c in components {
        buf.push(c);
    }
    buf.into_string()
}

/// Parent directory. Returns `None` for root and empty string.
pub fn parent(path: &str) -> Option<&str> {
    Utf8Path::new(path).parent().map(Utf8Path::as_str)
}

/// File name (last component). Returns `None` for root or empty.
pub fn filename(path: &str) -> Option<&str> {
    Utf8Path::new(path).file_name()
}

/// File stem (filename without extension).
pub fn stem(path: &str) -> Option<&str> {
    Utf8Path::new(path).file_stem()
}

/// File extension (without dot).
pub fn extension(path: &str) -> Option<&str> {
    Utf8Path::new(path).extension()
}

/// Replace extension. Empty `ext` removes it.
pub fn with_extension(path: &str, ext: &str) -> String {
    let mut buf = Utf8PathBuf::from(path);
    buf.set_extension(ext);
    buf.into_string()
}

/// Lexical normalization: resolve `.` and `..` without filesystem access.
pub fn normalize(path: &str) -> String {
    use path_clean::PathClean;
    // path-clean operates on std::path::Path. Round-trip is safe:
    // input is UTF-8, clean() only rearranges components.
    let std_path = Utf8Path::new(path).as_std_path();
    let cleaned = std_path.clean();
    cleaned
        .to_str()
        .expect("path-clean cannot introduce non-UTF-8 bytes from UTF-8 input")
        .to_string()
}

/// Compute relative path from `base` to `path`.
/// Returns `None` when no relative path exists.
pub fn relative(path: &str, base: &str) -> Option<String> {
    pathdiff::diff_utf8_paths(Utf8Path::new(path), Utf8Path::new(base)).map(|p| p.into_string())
}

/// Split path into components.
/// Root `/` appears as `"/"`, `.` and `..` appear literally.
pub fn components(path: &str) -> Vec<String> {
    Utf8Path::new(path)
        .components()
        .map(|c| c.as_str().to_string())
        .collect()
}

/// True if path is absolute.
pub fn is_absolute(path: &str) -> bool {
    Utf8Path::new(path).is_absolute()
}

/// True if path is relative.
pub fn is_relative(path: &str) -> bool {
    Utf8Path::new(path).is_relative()
}

// =============================================================================
// Filesystem operations
// =============================================================================

/// Current working directory.
pub fn cwd() -> Result<String, String> {
    std::env::current_dir()
        .map_err(|e| format!("failed to get current directory: {}", e))
        .and_then(|p| {
            p.to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| "current directory is not valid UTF-8".to_string())
        })
}

/// Compute absolute path: join with CWD if relative, then normalize.
/// Does not require path to exist.
pub fn absolute(path: &str) -> Result<String, String> {
    if is_absolute(path) {
        Ok(normalize(path))
    } else {
        let cwd = cwd()?;
        Ok(normalize(&join(&[&cwd, path])))
    }
}

/// Resolve path through the filesystem (symlinks resolved, must exist).
pub fn canonicalize(path: &str) -> Result<String, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("failed to resolve '{}': {}", path, e))
        .and_then(|p| {
            p.to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("resolved path for '{}' is not valid UTF-8", path))
        })
}

/// True if path exists (file, directory, or symlink target).
pub fn exists(path: &str) -> bool {
    Utf8Path::new(path).exists()
}

/// True if path exists and is a regular file.
pub fn is_file(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// True if path exists and is a directory.
pub fn is_dir(path: &str) -> bool {
    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

#[cfg(test)]
mod tests {
    use super::*;

    // --- join ---
    #[test]
    fn test_join_basic() {
        assert_eq!(join(&["a", "b", "c"]), "a/b/c");
    }

    #[test]
    fn test_join_single() {
        assert_eq!(join(&["hello"]), "hello");
    }

    #[test]
    fn test_join_absolute_replaces() {
        assert_eq!(join(&["a", "/b"]), "/b");
    }

    #[test]
    fn test_join_empty_components() {
        assert_eq!(join(&["a", "", "b"]), "a/b");
    }

    // --- parent ---
    #[test]
    fn test_parent_file() {
        assert_eq!(parent("/home/user/data.txt"), Some("/home/user"));
    }

    #[test]
    fn test_parent_root() {
        // Utf8Path::new("/").parent() returns None for the root path.
        let p = parent("/");
        assert_eq!(p, None);
    }

    #[test]
    fn test_parent_relative() {
        assert_eq!(parent("a/b/c"), Some("a/b"));
    }

    #[test]
    fn test_parent_single_component() {
        assert_eq!(parent("foo"), Some(""));
    }

    #[test]
    fn test_parent_empty() {
        assert_eq!(parent(""), None);
    }

    // --- filename ---
    #[test]
    fn test_filename_with_dir() {
        assert_eq!(filename("/home/user/data.txt"), Some("data.txt"));
    }

    #[test]
    fn test_filename_bare() {
        assert_eq!(filename("data.txt"), Some("data.txt"));
    }

    #[test]
    fn test_filename_trailing_slash() {
        assert_eq!(filename("/home/user/"), Some("user"));
    }

    // --- stem ---
    #[test]
    fn test_stem_basic() {
        assert_eq!(stem("data.txt"), Some("data"));
    }

    #[test]
    fn test_stem_multiple_dots() {
        assert_eq!(stem("archive.tar.gz"), Some("archive.tar"));
    }

    #[test]
    fn test_stem_no_extension() {
        assert_eq!(stem("noext"), Some("noext"));
    }

    // --- extension ---
    #[test]
    fn test_extension_basic() {
        assert_eq!(extension("data.txt"), Some("txt"));
    }

    #[test]
    fn test_extension_none() {
        assert_eq!(extension("noext"), None);
    }

    #[test]
    fn test_extension_multiple_dots() {
        assert_eq!(extension("archive.tar.gz"), Some("gz"));
    }

    // --- with_extension ---
    #[test]
    fn test_with_extension_replace() {
        assert_eq!(with_extension("foo.txt", "rs"), "foo.rs");
    }

    #[test]
    fn test_with_extension_add() {
        assert_eq!(with_extension("foo", "rs"), "foo.rs");
    }

    #[test]
    fn test_with_extension_remove() {
        assert_eq!(with_extension("foo.txt", ""), "foo");
    }

    // --- normalize ---
    #[test]
    fn test_normalize_dots() {
        assert_eq!(normalize("./a/../b"), "b");
    }

    #[test]
    fn test_normalize_absolute() {
        assert_eq!(normalize("/a/./b/../c"), "/a/c");
    }

    #[test]
    fn test_normalize_empty() {
        assert_eq!(normalize(""), ".");
    }

    // --- relative ---
    #[test]
    fn test_relative_subpath() {
        assert_eq!(
            relative("/foo/bar/baz", "/foo/bar"),
            Some("baz".to_string())
        );
    }

    #[test]
    fn test_relative_sibling() {
        let r = relative("/foo/bar", "/foo/baz");
        assert_eq!(r, Some("../bar".to_string()));
    }

    // --- components ---
    #[test]
    fn test_components_absolute() {
        assert_eq!(components("/a/b/c"), vec!["/", "a", "b", "c"]);
    }

    #[test]
    fn test_components_relative() {
        assert_eq!(components("a/b"), vec!["a", "b"]);
    }

    // --- is_absolute / is_relative ---
    #[test]
    fn test_is_absolute() {
        assert!(is_absolute("/foo"));
        assert!(!is_absolute("foo"));
    }

    #[test]
    fn test_is_relative() {
        assert!(is_relative("foo"));
        assert!(!is_relative("/foo"));
    }

    // --- filesystem operations ---
    #[test]
    fn test_exists() {
        assert!(exists("."));
        assert!(!exists("/nonexistent/xyz"));
    }

    #[test]
    fn test_is_dir() {
        assert!(is_dir("."));
        assert!(!is_dir("/nonexistent/xyz"));
    }

    #[test]
    fn test_is_file_on_dir() {
        assert!(!is_file("."));
    }

    #[test]
    fn test_cwd_nonempty() {
        let c = cwd().unwrap();
        assert!(!c.is_empty());
    }

    #[test]
    fn test_absolute_relative_path() {
        let abs = absolute("src").unwrap();
        assert!(is_absolute(&abs));
    }

    #[test]
    fn test_absolute_already_absolute() {
        let abs = absolute("/tmp").unwrap();
        assert_eq!(abs, "/tmp");
    }

    #[test]
    fn test_canonicalize_dot() {
        let c = canonicalize(".").unwrap();
        assert!(is_absolute(&c));
    }

    #[test]
    fn test_canonicalize_nonexistent() {
        assert!(canonicalize("/nonexistent/xyz").is_err());
    }
}
