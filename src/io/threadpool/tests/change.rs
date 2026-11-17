// audited: 2026-11-17
//! What a pool worker reports for a change to a path's metadata, a temporary directory's name, and a missing path.
//!
//! docs/io/files.md

use super::super::meta::{make_temp_dir, temp_candidate};
use super::file::run_file_op;
use super::meta::ok;
use crate::io::request::FileOp;
use crate::scratch::ScratchDir;
use std::sync::atomic::{AtomicU64, Ordering};

#[test]
fn a_mkdir_makes_one_level_and_a_mkdir_all_makes_every_level() {
    let dir = ScratchDir::new("tp-mkdir");
    let one = dir.join_str("one");
    ok(FileOp::Mkdir {
        path: one.clone(),
        parents: false,
    });
    assert!(std::path::Path::new(&one).is_dir());
    let again = FileOp::Mkdir {
        path: one,
        parents: false,
    };
    assert_eq!(run_file_op(1, again).0, -libc::EEXIST);

    let deep = dir.join_str("a/b/c");
    let shallow = FileOp::Mkdir {
        path: deep.clone(),
        parents: false,
    };
    assert_eq!(
        run_file_op(1, shallow).0,
        -libc::ENOENT,
        "no parent, no mkdir"
    );
    ok(FileOp::Mkdir {
        path: deep.clone(),
        parents: true,
    });
    assert!(std::path::Path::new(&deep).is_dir());
}

#[test]
fn a_delete_a_delete_dir_and_a_rename_change_what_is_there() {
    let dir = ScratchDir::new("tp-change");
    let file = dir.join_str("file");
    std::fs::write(&file, "x").unwrap();
    let moved = dir.join_str("moved");
    ok(FileOp::Rename {
        from: file.clone(),
        to: moved.clone(),
    });
    assert!(!std::path::Path::new(&file).exists());
    assert_eq!(std::fs::read_to_string(&moved).unwrap(), "x");
    ok(FileOp::Delete {
        path: moved.clone(),
    });
    assert!(!std::path::Path::new(&moved).exists());

    let tree = dir.join_str("tree");
    std::fs::create_dir_all(format!("{tree}/leaf")).unwrap();
    let not_empty = FileOp::DeleteDir {
        path: tree.clone(),
        recursive: false,
    };
    assert_eq!(run_file_op(1, not_empty).0, -libc::ENOTEMPTY);
    ok(FileOp::DeleteDir {
        path: format!("{tree}/leaf"),
        recursive: false,
    });
    std::fs::write(format!("{tree}/f"), "").unwrap();
    ok(FileOp::DeleteDir {
        path: tree.clone(),
        recursive: true,
    });
    assert!(!std::path::Path::new(&tree).exists());
}

#[test]
fn a_temp_dir_is_made_under_its_root_and_named_for_this_process() {
    let root = ScratchDir::new("tp-tmproot");
    let (_, data) = ok(FileOp::MakeTempDir {
        root: root.as_str().to_string(),
    });
    let made = String::from_utf8(data).unwrap();
    let path = std::path::Path::new(&made);
    assert!(path.is_dir(), "{made} is a directory");
    assert_eq!(path.parent(), Some(std::path::Path::new(root.as_str())));
    let prefix = format!("elle-{}-", std::process::id());
    assert!(path
        .file_name()
        .unwrap()
        .to_str()
        .unwrap()
        .starts_with(&prefix));
}

/// A candidate that exists already is skipped, and the next one taken: a
/// recycled pid can meet a directory a dead process left.
#[test]
fn a_temp_dir_skips_a_name_already_taken() {
    let root = ScratchDir::new("tp-tmpskip");
    let counter = AtomicU64::new(0);
    std::fs::create_dir(temp_candidate(root.as_str(), 0)).unwrap();
    let (code, data) = make_temp_dir(root.as_str(), &counter);
    assert_eq!(code, 0, "{}", String::from_utf8_lossy(&data));
    assert_eq!(
        String::from_utf8(data).unwrap(),
        temp_candidate(root.as_str(), 1)
    );
    assert_eq!(counter.load(Ordering::Relaxed), 2, "two candidates taken");
}

/// The search ends after 1024 names, with `EEXIST`, rather than running on.
/// Any other failure ends it at once.
#[test]
fn a_temp_dir_gives_up_after_1024_names_and_at_any_other_failure() {
    let root = ScratchDir::new("tp-tmpfull");
    for n in 0..1024 {
        std::fs::create_dir(temp_candidate(root.as_str(), n)).unwrap();
    }
    let counter = AtomicU64::new(0);
    assert_eq!(make_temp_dir(root.as_str(), &counter).0, -libc::EEXIST);
    assert_eq!(counter.load(Ordering::Relaxed), 1024);
    assert!(!std::path::Path::new(&temp_candidate(root.as_str(), 1024)).exists());

    let counter = AtomicU64::new(0);
    let missing = root.join_str("missing");
    assert_eq!(make_temp_dir(&missing, &counter).0, -libc::ENOENT);
    assert_eq!(counter.load(Ordering::Relaxed), 1, "one attempt, then stop");
}

#[test]
fn an_operation_on_a_missing_path_reports_enoent_and_the_reason() {
    let dir = ScratchDir::new("tp-meta-missing");
    let missing = dir.join_str("nothing/here");
    let ops = [
        FileOp::Stat {
            path: missing.clone(),
            follow: true,
        },
        FileOp::Stat {
            path: missing.clone(),
            follow: false,
        },
        FileOp::Size {
            path: missing.clone(),
        },
        FileOp::List {
            path: missing.clone(),
        },
        FileOp::Delete {
            path: missing.clone(),
        },
        FileOp::DeleteDir {
            path: missing.clone(),
            recursive: false,
        },
        FileOp::DeleteDir {
            path: missing.clone(),
            recursive: true,
        },
        FileOp::Rename {
            from: missing.clone(),
            to: dir.join_str("to"),
        },
        FileOp::Canonicalize {
            path: missing.clone(),
        },
    ];
    for op in ops {
        let kind = op.kind();
        let (code, why) = run_file_op(1, op);
        assert_eq!(code, -libc::ENOENT, "{kind:?}");
        assert!(!why.is_empty(), "{kind:?} says why");
    }
}
