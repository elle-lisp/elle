//! Module resolution for `import`.
//!
//! Resolves bare names (`"regex"`), relative paths (`"./utils"`),
//! and absolute paths (`"/opt/elle/lib/foo.lisp"`) to filesystem paths.
//! Separate from the VM — pure path arithmetic plus filesystem existence checks.

use crate::path;

/// Specifier classification.
#[derive(Debug, PartialEq)]
pub(crate) enum Specifier {
    /// `./foo` or `../foo` — relative to importing file's directory.
    Relative,
    /// `/opt/elle/lib/foo.lisp` — used as-is.
    Absolute,
    /// `regex`, `http` — searched via ELLE_PATH.
    Bare,
}

/// Classify an import specifier.
pub(crate) fn classify(spec: &str) -> Specifier {
    if spec.starts_with('/') {
        Specifier::Absolute
    } else if spec.starts_with("./") || spec.starts_with("../") {
        Specifier::Relative
    } else {
        Specifier::Bare
    }
}

/// Known extensions that skip candidate generation.
fn has_known_extension(spec: &str) -> bool {
    matches!(
        path::extension(spec),
        Some("lisp" | "elle" | "so" | "dylib")
    )
}

/// Build candidate filenames for a bare name without extension.
///
/// For `"foo"`, returns `["foo.elle", "foo.lisp", "libelle_foo.so"]`
/// (or `.dylib` on macOS).
fn candidates(name: &str) -> Vec<String> {
    let mut out = vec![format!("{}.elle", name), format!("{}.lisp", name)];
    out.extend(native_candidates(name));
    out
}

/// Build candidate filenames for a native plugin only.
///
/// For `"foo"`, returns `["libelle_foo.so"]` (or `.dylib` on macOS).
pub(crate) fn native_candidates(name: &str) -> Vec<String> {
    if cfg!(target_os = "macos") {
        vec![format!("libelle_{}.dylib", name)]
    } else {
        vec![format!("libelle_{}.so", name)]
    }
}

/// Compute ELLE_HOME from env or binary location.
///
/// When `ELLE_HOME` is set, returns that value.
/// Otherwise derives from the `elle` binary's path:
/// - If binary is `$DIR/target/{release,debug}/elle` → `$DIR` (dev mode)
/// - Otherwise `parent(parent(binary))` (installed: `$PREFIX/bin/elle` → `$PREFIX`)
pub(crate) fn elle_home() -> Option<String> {
    if let Ok(val) = std::env::var("ELLE_HOME") {
        if !val.is_empty() {
            return Some(val);
        }
    }

    let exe = std::env::current_exe().ok()?;
    let exe_str = exe.to_str()?;

    // Dev mode: .../target/{release,debug}/elle
    let parent_dir = path::parent(exe_str)?;
    let parent_name = path::filename(parent_dir)?;
    if parent_name == "release" || parent_name == "debug" {
        let target_dir = path::parent(parent_dir)?;
        if path::filename(target_dir)? == "target" {
            return path::parent(target_dir).map(|s| s.to_string());
        }
    }

    // Installed mode: $PREFIX/bin/elle → $PREFIX
    path::parent(parent_dir).map(|s| s.to_string())
}

/// Compute the effective search path.
///
/// When `ELLE_PATH` is set, splits on `:` and returns those directories.
/// Otherwise returns `[$ELLE_HOME/lib, $ELLE_HOME/target/$SELF, $ELLE_HOME/target/$OTHER]`
/// where `$SELF` is the current build profile and `$OTHER` is the alternate.
pub(crate) fn search_path() -> Vec<String> {
    if let Ok(val) = std::env::var("ELLE_PATH") {
        if !val.is_empty() {
            return val.split(':').map(|s| s.to_string()).collect();
        }
    }

    let home = match elle_home() {
        Some(h) => h,
        None => return Vec::new(),
    };

    let (self_profile, other_profile) = if cfg!(debug_assertions) {
        ("debug", "release")
    } else {
        ("release", "debug")
    };

    vec![
        path::join(&[&home, "lib"]),
        path::join(&[&home, "target", self_profile]),
        path::join(&[&home, "target", other_profile]),
    ]
}

/// Resolve an import specifier to a filesystem path.
///
/// - `caller_dir`: directory of the importing file (for relative imports)
/// - `search_dirs`: directories to search (for bare names)
///
/// Returns `Some(path)` if found, `None` otherwise.
pub(crate) fn resolve(
    spec: &str,
    caller_dir: Option<&str>,
    search_dirs: &[String],
) -> Option<String> {
    match classify(spec) {
        Specifier::Absolute => {
            if has_known_extension(spec) {
                if path::exists(spec) {
                    return Some(spec.to_string());
                }
                return None;
            }
            // Try candidates at absolute path's parent
            let dir = path::parent(spec)?;
            let name = path::filename(spec)?;
            for candidate in candidates(name) {
                let full = path::join(&[dir, &candidate]);
                if path::exists(&full) {
                    return Some(full);
                }
            }
            None
        }
        Specifier::Relative => {
            let base = caller_dir?;
            if has_known_extension(spec) {
                let full = path::join(&[base, spec]);
                let normalized = path::normalize(&full);
                if path::exists(&normalized) {
                    return Some(normalized);
                }
                return None;
            }
            let name = path::filename(spec).unwrap_or(spec);
            // Resolve the relative directory component
            let rel_dir = path::parent(spec).unwrap_or(".");
            let search_dir = path::normalize(&path::join(&[base, rel_dir]));
            for candidate in candidates(name) {
                let full = path::join(&[&search_dir, &candidate]);
                if path::exists(&full) {
                    return Some(full);
                }
            }
            None
        }
        Specifier::Bare => {
            for dir in search_dirs {
                if has_known_extension(spec) {
                    let full = path::join(&[dir.as_str(), spec]);
                    if path::exists(&full) {
                        return Some(full);
                    }
                } else {
                    for candidate in candidates(spec) {
                        let full = path::join(&[dir.as_str(), &candidate]);
                        if path::exists(&full) {
                            return Some(full);
                        }
                    }
                }
            }
            None
        }
    }
}

/// Resolve a native plugin by name. Only searches for `libelle_NAME.{so,dylib}`.
pub(crate) fn resolve_native(name: &str, search_dirs: &[String]) -> Option<String> {
    for dir in search_dirs {
        for candidate in native_candidates(name) {
            let full = path::join(&[dir.as_str(), &candidate]);
            if path::exists(&full) {
                return Some(full);
            }
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn classify_bare() {
        assert_eq!(classify("regex"), Specifier::Bare);
        assert_eq!(classify("http"), Specifier::Bare);
        assert_eq!(classify("tree_sitter"), Specifier::Bare);
    }

    #[test]
    fn classify_relative() {
        assert_eq!(classify("./utils"), Specifier::Relative);
        assert_eq!(classify("../lib/foo"), Specifier::Relative);
        assert_eq!(classify("./foo.lisp"), Specifier::Relative);
    }

    #[test]
    fn classify_absolute() {
        assert_eq!(classify("/opt/elle/lib/http.lisp"), Specifier::Absolute);
        assert_eq!(classify("/usr/lib/libelle_regex.so"), Specifier::Absolute);
    }

    #[test]
    fn candidates_linux_or_macos() {
        let c = candidates("regex");
        assert_eq!(c[0], "regex.elle");
        assert_eq!(c[1], "regex.lisp");
        // Third is platform-specific
        if cfg!(target_os = "macos") {
            assert_eq!(c[2], "libelle_regex.dylib");
        } else {
            assert_eq!(c[2], "libelle_regex.so");
        }
    }

    #[test]
    fn elle_home_from_env() {
        // Save and restore
        let saved = std::env::var("ELLE_HOME").ok();
        unsafe { std::env::set_var("ELLE_HOME", "/test/elle") };
        assert_eq!(elle_home(), Some("/test/elle".to_string()));
        match saved {
            Some(v) => unsafe { std::env::set_var("ELLE_HOME", v) },
            None => unsafe { std::env::remove_var("ELLE_HOME") },
        }
    }

    #[test]
    fn search_path_from_env() {
        let saved = std::env::var("ELLE_PATH").ok();
        unsafe { std::env::set_var("ELLE_PATH", "/a:/b:/c") };
        let dirs = search_path();
        assert_eq!(dirs, vec!["/a", "/b", "/c"]);
        match saved {
            Some(v) => unsafe { std::env::set_var("ELLE_PATH", v) },
            None => unsafe { std::env::remove_var("ELLE_PATH") },
        }
    }

    #[test]
    fn resolve_finds_existing_file() {
        // Create a temp dir with a test file
        let dir = std::env::temp_dir().join("elle_resolve_test");
        let _ = std::fs::create_dir_all(&dir);
        let test_file = dir.join("testmod.lisp");
        std::fs::write(&test_file, "()").unwrap();

        let dirs = vec![dir.to_str().unwrap().to_string()];
        let result = resolve("testmod", None, &dirs);
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("testmod.lisp"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_returns_none_for_missing() {
        let dirs = vec!["/nonexistent".to_string()];
        assert_eq!(resolve("no_such_module_xyz", None, &dirs), None);
    }

    #[test]
    fn resolve_relative_with_caller_dir() {
        let dir = std::env::temp_dir().join("elle_resolve_rel_test");
        let _ = std::fs::create_dir_all(&dir);
        let test_file = dir.join("helper.lisp");
        std::fs::write(&test_file, "()").unwrap();

        let caller = dir.to_str().unwrap();
        let result = resolve("./helper", Some(caller), &[]);
        assert!(result.is_some());
        assert!(result.unwrap().ends_with("helper.lisp"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn resolve_with_explicit_extension() {
        let dir = std::env::temp_dir().join("elle_resolve_ext_test");
        let _ = std::fs::create_dir_all(&dir);
        let test_file = dir.join("mod.elle");
        std::fs::write(&test_file, "()").unwrap();

        let dirs = vec![dir.to_str().unwrap().to_string()];
        let result = resolve("mod.elle", None, &dirs);
        assert!(result.is_some());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
