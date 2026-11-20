// audited: 2026-11-20
//! What a pool worker's canonicalize answers: what `std::fs::canonicalize` answers, case by case, and EINVAL for a path that is not UTF-8.
//!
//! docs/io/files.md

use super::file::run_file_op;
use crate::io::request::FileOp;
use crate::scratch::ScratchDir;
use std::os::unix::fs::symlink;

/// What `std` answers for `path`: 0 and the path, or `-errno`.
fn std_answer(path: &str) -> (i32, String) {
    match std::fs::canonicalize(path) {
        Ok(p) => (0, p.into_os_string().into_string().unwrap()),
        Err(e) => (-e.raw_os_error().unwrap(), String::new()),
    }
}

/// What the worker answers for `path`: 0 and the path, or `-errno`.
fn ours(path: &str) -> (i32, String) {
    let (code, data) = run_file_op(
        50,
        FileOp::Canonicalize {
            path: path.to_string(),
        },
    );
    if code < 0 {
        assert!(!data.is_empty(), "{path:?}: a failure says why");
        return (code, String::new());
    }
    (code, String::from_utf8(data).unwrap())
}

/// A chain of `n` symlinks under `dir`, each naming the next by a relative
/// path, the last naming `target`. Answers the first.
fn chain(dir: &ScratchDir, tag: &str, n: usize, target: &str) -> String {
    let link = |i: usize| dir.join_str(&format!("{tag}{i}"));
    symlink(target, link(n - 1)).unwrap();
    for i in (0..n - 1).rev() {
        symlink(format!("{tag}{}", i + 1), link(i)).unwrap();
    }
    link(0)
}

/// Each path resolves as `std` resolves it. The cases are the edges of a
/// hand-written resolution. They hold a `..` after a link, a link to a link,
/// and the loop limit on each side of the count Linux and macOS use. They hold
/// a component that is not a directory, a trailing slash on a file, and a
/// missing component. A relative path starts from the working directory.
#[test]
fn a_canonical_path_is_what_std_resolves() {
    let dir = ScratchDir::new("tp-canon-std");
    let d = |p: &str| dir.join_str(p);
    std::fs::create_dir_all(d("real/sub")).unwrap();
    std::fs::write(d("real/sub/file"), "").unwrap();
    symlink(d("real/sub"), d("abs")).unwrap();
    symlink("real/sub", d("rel")).unwrap();
    symlink("rel", d("hop")).unwrap();
    symlink("../real", d("real/sub/up")).unwrap();
    symlink("nowhere", d("dangling")).unwrap();
    symlink("loop", d("loop")).unwrap();
    let mut cases: Vec<String> = [
        "real",
        "real/sub/file",
        "real/./sub//file",
        "real/sub/../sub/file",
        "abs/file",
        "abs/..",
        "rel/../real",
        "hop/file",
        "real/sub/up/sub/up",
        "dangling",
        "loop",
        "loop/x",
        "real/sub/file/",
        "real/sub/file/.",
        "real/sub/file/x",
        "real/sub/file/..",
        "missing",
        "missing/..",
        "real/missing/x",
    ]
    .iter()
    .map(|p| d(p))
    .collect();
    for n in [31, 32, 33, 39, 40, 41] {
        cases.push(chain(&dir, &format!("c{n}-"), n, &d("real/sub/file")));
    }
    for abs in ["/", "/..", "//", "/.", ""] {
        cases.push(abs.to_string());
    }
    for rel in [".", "src", "src/../Cargo.toml", "no-such-file"] {
        cases.push(rel.to_string());
    }
    for path in cases {
        assert_eq!(ours(&path), std_answer(&path), "{path:?}");
    }
}

/// A path that resolves to bytes that are not UTF-8 fails with EINVAL, since an
/// Elle string cannot hold it. `std` resolves it, which is the control.
#[test]
fn a_path_that_resolves_to_bytes_that_are_not_utf8_fails() {
    use std::os::unix::ffi::OsStrExt;
    let dir = ScratchDir::new("tp-canon-utf8");
    let raw = std::path::Path::new(dir.as_str()).join(std::ffi::OsStr::from_bytes(b"\xff"));
    std::fs::create_dir(&raw).unwrap();
    let link = dir.join_str("link");
    symlink(&raw, &link).unwrap();
    assert!(std::fs::canonicalize(&link).is_ok(), "std resolves it");
    let (code, why) = run_file_op(51, FileOp::Canonicalize { path: link.clone() });
    assert_eq!(code, -libc::EINVAL, "{}", String::from_utf8_lossy(&why));
}
