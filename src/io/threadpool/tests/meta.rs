// audited: 2026-11-17
//! What a pool worker reports for a read of a path's metadata: a stat, a size, a list, a probe and a canonical path.
//!
//! docs/io/files.md

use super::file::run_file_op;
use super::Fifo;
use crate::io::request::{FileOp, FileType, PathTest, StatRecord};
use crate::scratch::ScratchDir;

/// Run `op`, assert it succeeded, and answer its result code and bytes.
pub(super) fn ok(op: FileOp) -> (i32, Vec<u8>) {
    let (code, data) = run_file_op(1, op);
    assert!(
        code >= 0,
        "failed with {code}: {}",
        String::from_utf8_lossy(&data)
    );
    (code, data)
}

fn stat(path: &str, follow: bool) -> StatRecord {
    let (_, data) = ok(FileOp::Stat {
        path: path.to_string(),
        follow,
    });
    StatRecord::decode(&data).expect("a stat reports one encoded record")
}

fn probe(path: &str, test: PathTest) -> i32 {
    let (code, data) = ok(FileOp::Probe {
        path: path.to_string(),
        test,
    });
    assert!(data.is_empty(), "a probe answers in its result code alone");
    code
}

#[test]
fn a_stat_reports_the_record_of_what_the_path_names() {
    let dir = ScratchDir::new("tp-stat");
    let file = dir.join_str("file");
    std::fs::write(&file, "hello").unwrap();
    let link = dir.join_str("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();

    let rec = stat(&link, true);
    assert_eq!(
        (rec.file_type, rec.size),
        (FileType::File, 5),
        "a stat follows"
    );
    assert_eq!(
        stat(&link, false).file_type,
        FileType::Symlink,
        "an lstat does not"
    );
    assert_eq!(stat(dir.as_str(), true).file_type, FileType::Dir);
}

#[test]
fn a_size_reports_the_length_as_eight_little_endian_bytes() {
    let dir = ScratchDir::new("tp-size");
    let file = dir.join_str("file");
    std::fs::write(&file, "hello").unwrap();
    let (_, data) = ok(FileOp::Size { path: file });
    assert_eq!(data, 5u64.to_le_bytes().to_vec());
}

/// A list reports each name followed by a NUL, which no name holds, and
/// leaves out a name that is not UTF-8, as `file/ls` always has.
#[test]
fn a_list_reports_each_utf8_name_followed_by_a_nul() {
    use std::os::unix::ffi::OsStrExt;
    let dir = ScratchDir::new("tp-list");
    let (_, empty) = ok(FileOp::List {
        path: dir.as_str().to_string(),
    });
    assert!(empty.is_empty(), "an empty directory reports no bytes");

    std::fs::write(dir.join_str("a"), "").unwrap();
    std::fs::create_dir(dir.join_str("b")).unwrap();
    let raw = std::path::Path::new(dir.as_str()).join(std::ffi::OsStr::from_bytes(b"\xff"));
    std::fs::write(raw, "").unwrap();
    let (_, data) = ok(FileOp::List {
        path: dir.as_str().to_string(),
    });
    assert_eq!(data.last(), Some(&0), "the last name ends in a NUL too");
    let mut names: Vec<&[u8]> = data[..data.len() - 1].split(|b| *b == 0).collect();
    names.sort();
    assert_eq!(names, vec![&b"a"[..], &b"b"[..]]);
}

#[test]
fn a_probe_answers_one_or_zero_and_never_fails() {
    let dir = ScratchDir::new("tp-probe");
    let file = dir.join_str("file");
    std::fs::write(&file, "").unwrap();
    let dangling = dir.join_str("dangling");
    std::os::unix::fs::symlink(dir.join_str("nothing"), &dangling).unwrap();
    let missing = dir.join_str("missing");
    let cases = [
        (file.as_str(), [1, 1, 0]),
        (dir.as_str(), [1, 0, 1]),
        (missing.as_str(), [0, 0, 0]),
        (dangling.as_str(), [0, 0, 0]),
    ];
    for (path, want) in cases {
        let got = [PathTest::Exists, PathTest::File, PathTest::Dir].map(|t| probe(path, t));
        assert_eq!(got, want, "{path}: exists, file, dir");
    }
}

#[test]
fn a_canonical_path_resolves_every_link_and_dot() {
    let dir = ScratchDir::new("tp-canon");
    std::fs::create_dir(dir.join_str("real")).unwrap();
    std::fs::write(dir.join_str("real/file"), "").unwrap();
    std::os::unix::fs::symlink(dir.join_str("real"), dir.join_str("link")).unwrap();
    let want = std::fs::canonicalize(dir.join_str("real/file")).unwrap();
    let (_, data) = ok(FileOp::Canonicalize {
        path: dir.join_str("link/./../link/file"),
    });
    assert_eq!(String::from_utf8(data).unwrap(), want.to_str().unwrap());
}

/// The trap: an open of a fifo waits for its other end, and nobody opens this
/// one. A read of its metadata opens nothing, so each of these reports back
/// at once, where an open would park the worker past `run_file_op`'s wait.
#[test]
fn a_fifo_is_stated_probed_and_listed_at_once() {
    let fifo = Fifo::new("meta-fifo");
    let rec = stat(&fifo.0, true);
    assert_eq!(
        rec.file_type,
        FileType::Other,
        "a fifo is neither file nor dir"
    );
    assert_eq!(probe(&fifo.0, PathTest::Exists), 1);
    assert_eq!(probe(&fifo.0, PathTest::File), 0);
    let (_, size) = ok(FileOp::Size {
        path: fifo.0.clone(),
    });
    assert_eq!(size, 0u64.to_le_bytes().to_vec());

    let dir = ScratchDir::new("tp-meta-fifo-dir");
    let c_path = std::ffi::CString::new(dir.join_str("fifo")).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c_path.as_ptr(), 0o600) }, 0);
    let (_, names) = ok(FileOp::List {
        path: dir.as_str().to_string(),
    });
    assert_eq!(names, b"fifo\0".to_vec());
}
