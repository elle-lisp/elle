// audited: 2026-11-20
//! What a pool worker's tree delete and mkdir-all leave on disk: what `std` leaves, and never a symlink's target removed.
//!
//! docs/io/files.md

use super::file::run_file_op;
use crate::io::request::FileOp;
use crate::scratch::ScratchDir;
use std::path::Path;

/// Every path under `root`, relative to it and sorted, without following a
/// symlink.
fn tree(root: &str) -> Vec<String> {
    fn walk(root: &Path, dir: &Path, out: &mut Vec<String>) {
        for e in std::fs::read_dir(dir).unwrap() {
            let path = e.unwrap().path();
            out.push(path.strip_prefix(root).unwrap().display().to_string());
            if std::fs::symlink_metadata(&path).unwrap().is_dir() {
                walk(root, &path, out);
            }
        }
    }
    let mut out = Vec::new();
    walk(Path::new(root), Path::new(root), &mut out);
    out.sort();
    out
}

/// The code `std` reports: 0, or `-errno`.
fn code(r: std::io::Result<()>) -> i32 {
    match r {
        Ok(()) => 0,
        Err(e) => -e.raw_os_error().unwrap_or(libc::EINVAL),
    }
}

/// A directory outside the tree, with a file in it, that a symlink inside the
/// tree names.
fn outside(dir: &ScratchDir) -> String {
    let out = dir.join_str("outside");
    std::fs::create_dir(&out).unwrap();
    std::fs::write(format!("{out}/keep"), "kept").unwrap();
    out
}

fn delete_all(path: &str) -> i32 {
    run_file_op(
        40,
        FileOp::DeleteDir {
            path: path.to_string(),
            recursive: true,
        },
    )
    .0
}

/// A tree of every kind of entry goes, and a symlink inside it goes as a link.
///
/// The counter-factual follows the link: the delete then empties the directory
/// outside the tree, which nobody asked it to touch.
#[test]
fn a_delete_removes_every_entry_and_never_what_a_symlink_names() {
    use std::os::unix::fs::symlink;
    let dir = ScratchDir::new("tp-tree-delete");
    let out = outside(&dir);
    let t = dir.join_str("tree");
    std::fs::create_dir_all(format!("{t}/a/b/c")).unwrap();
    std::fs::create_dir(format!("{t}/empty")).unwrap();
    std::fs::write(format!("{t}/a/file"), "x").unwrap();
    std::fs::write(format!("{t}/a/b/c/deep"), "x").unwrap();
    symlink(&out, format!("{t}/a/to-dir")).unwrap();
    symlink(format!("{out}/keep"), format!("{t}/to-file")).unwrap();
    symlink(format!("{t}/nowhere"), format!("{t}/dangling")).unwrap();
    let fifo = std::ffi::CString::new(format!("{t}/a/b/fifo")).unwrap();
    assert_eq!(unsafe { libc::mkfifo(fifo.as_ptr(), 0o600) }, 0);

    assert_eq!(delete_all(&t), 0);
    assert!(!Path::new(&t).exists(), "the tree is gone");
    assert_eq!(
        std::fs::read_to_string(format!("{out}/keep")).unwrap(),
        "kept"
    );
}

/// A path that is itself a symlink to a directory loses the link, as `std`
/// removes it, and the directory keeps what it holds. A regular file fails as
/// `std` fails it.
#[test]
fn a_delete_of_a_symlink_removes_the_link_and_of_a_file_fails() {
    let dir = ScratchDir::new("tp-tree-top");
    let out = outside(&dir);
    let link = dir.join_str("link");
    std::os::unix::fs::symlink(&out, &link).unwrap();
    assert_eq!(delete_all(&link), 0);
    assert!(
        std::fs::symlink_metadata(&link).is_err(),
        "the link is gone"
    );
    assert_eq!(tree(&out), vec!["keep"], "and what it named is not");

    let file = format!("{out}/keep");
    let want = code(std::fs::remove_dir_all(&file));
    assert_ne!(want, 0, "std refuses a file");
    assert_eq!(delete_all(&file), want);
    assert!(Path::new(&file).exists());
}

/// Each case runs against a twin of the same layout: `std::fs::create_dir_all`
/// in one, the worker's mkdir-all in the other. Both answer the same code and
/// leave the same tree.
#[test]
fn a_mkdir_all_answers_and_makes_what_std_does() {
    let cases = [
        "new",
        "a/b/c",
        "a/b/c/",
        "a/./b",
        "a/../b",
        "dir",
        "dir/x/y",
        "file",
        "file/x",
        "link-to-dir/x",
        "link-to-file/x",
        "",
    ];
    for case in cases {
        let twins = [
            ScratchDir::new("tp-mkdir-std"),
            ScratchDir::new("tp-mkdir-ours"),
        ];
        for t in &twins {
            std::fs::create_dir(t.join_str("dir")).unwrap();
            std::fs::write(t.join_str("file"), "").unwrap();
            std::os::unix::fs::symlink(t.join_str("dir"), t.join_str("link-to-dir")).unwrap();
            std::os::unix::fs::symlink(t.join_str("file"), t.join_str("link-to-file")).unwrap();
        }
        let path = |t: &ScratchDir| {
            if case.is_empty() {
                String::new()
            } else {
                t.join_str(case)
            }
        };
        let want = code(std::fs::create_dir_all(path(&twins[0])));
        let (got, _) = run_file_op(
            41,
            FileOp::Mkdir {
                path: path(&twins[1]),
                parents: true,
            },
        );
        assert_eq!(got, want, "{case:?}: the code");
        assert_eq!(
            tree(twins[1].as_str()),
            tree(twins[0].as_str()),
            "{case:?}: the tree"
        );
    }
}
