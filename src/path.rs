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
pub(crate) fn stem(path: &str) -> Option<&str> {
    Utf8Path::new(path).file_stem()
}

/// File extension (without dot).
pub(crate) fn extension(path: &str) -> Option<&str> {
    Utf8Path::new(path).extension()
}

/// Replace extension. Empty `ext` removes it.
pub(crate) fn with_extension(path: &str, ext: &str) -> String {
    let mut buf = Utf8PathBuf::from(path);
    buf.set_extension(ext);
    buf.into_string()
}

/// Lexical normalization: resolve `.` and `..` without filesystem access.
pub(crate) fn normalize(path: &str) -> String {
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
pub(crate) fn relative(path: &str, base: &str) -> Option<String> {
    pathdiff::diff_utf8_paths(Utf8Path::new(path), Utf8Path::new(base)).map(|p| p.into_string())
}

/// Split path into components.
/// Root `/` appears as `"/"`, `.` and `..` appear literally.
pub(crate) fn components(path: &str) -> Vec<String> {
    Utf8Path::new(path)
        .components()
        .map(|c| c.as_str().to_string())
        .collect()
}

/// True if path is absolute.
pub(crate) fn is_absolute(path: &str) -> bool {
    Utf8Path::new(path).is_absolute()
}

/// True if path is relative.
pub(crate) fn is_relative(path: &str) -> bool {
    Utf8Path::new(path).is_relative()
}

// =============================================================================
// Filesystem operations
// =============================================================================

/// Current working directory.
pub(crate) fn cwd() -> Result<String, String> {
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
pub(crate) fn absolute(path: &str) -> Result<String, String> {
    if is_absolute(path) {
        Ok(normalize(path))
    } else {
        let cwd = cwd()?;
        Ok(normalize(&join(&[&cwd, path])))
    }
}

/// Resolve path through the filesystem (symlinks resolved, must exist).
pub(crate) fn canonicalize(path: &str) -> Result<String, String> {
    std::fs::canonicalize(path)
        .map_err(|e| format!("failed to resolve '{}': {}", path, e))
        .and_then(|p| {
            p.to_str()
                .map(|s| s.to_string())
                .ok_or_else(|| format!("resolved path for '{}' is not valid UTF-8", path))
        })
}

/// True if path exists (file, directory, or symlink target).
pub(crate) fn exists(path: &str) -> bool {
    Utf8Path::new(path).exists()
}

/// True if path exists and is a regular file.
pub(crate) fn is_file(path: &str) -> bool {
    std::fs::metadata(path)
        .map(|m| m.is_file())
        .unwrap_or(false)
}

/// True if path exists and is a directory.
pub(crate) fn is_dir(path: &str) -> bool {
    std::fs::metadata(path).map(|m| m.is_dir()).unwrap_or(false)
}

#[cfg(test)]
mod tests;
