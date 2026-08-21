//! Unit tests (`super` is the parent impl module).

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
    let abs = absolute("/usr").unwrap();
    assert_eq!(abs, "/usr");
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
