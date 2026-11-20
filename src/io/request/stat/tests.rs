// audited: 2026-11-20
//! A stat record says what the OS reported, and survives its encoding whole.
//!
//! docs/io/files.md

use super::*;
use crate::scratch::ScratchDir;
use std::os::unix::fs::{MetadataExt, PermissionsExt};

#[test]
fn a_record_says_what_the_path_names() {
    let dir = ScratchDir::new("stat-names");
    let file = dir.join("file");
    std::fs::write(&file, "hello").unwrap();
    std::fs::set_permissions(&file, std::fs::Permissions::from_mode(0o444)).unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();

    let meta = std::fs::metadata(&file).unwrap();
    let rec = StatRecord::from_metadata(&meta);
    assert_eq!(rec.file_type, FileType::File);
    assert_eq!(rec.size, 5);
    assert_eq!(rec.permissions & 0o777, 0o444);
    assert!(rec.readonly(), "no write bit is set");
    assert_eq!(rec.inode, meta.ino());
    assert_eq!(rec.nlinks, 1);
    assert_eq!(rec.uid, u64::from(meta.uid()));
    assert!(
        rec.modified.is_some(),
        "every filesystem keeps a modified time"
    );

    let dir_rec = StatRecord::from_metadata(&std::fs::metadata(dir.as_str()).unwrap());
    assert_eq!(dir_rec.file_type, FileType::Dir);
    assert!(!dir_rec.readonly(), "the owner may write the directory");

    let followed = StatRecord::from_metadata(&std::fs::metadata(&link).unwrap());
    assert_eq!(
        followed.file_type,
        FileType::File,
        "a stat follows the link"
    );
    let link_rec = StatRecord::from_metadata(&std::fs::symlink_metadata(&link).unwrap());
    assert_eq!(link_rec.file_type, FileType::Symlink, "an lstat does not");
}

/// Decoding takes exactly one whole record: the record back from its own
/// encoding, and `None` from anything shorter or longer.
///
/// The counter-factual: a decode that read words while bytes lasted would
/// answer a record from a truncated buffer, its last fields zero, and the
/// stat struct would report a size or an inode nobody measured.
#[test]
fn a_record_survives_its_encoding_and_nothing_else_decodes() {
    let dir = ScratchDir::new("stat-codec");
    let file = dir.join("file");
    std::fs::write(&file, "some bytes").unwrap();
    let link = dir.join("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();

    let records = [
        StatRecord::from_metadata(&std::fs::metadata(&file).unwrap()),
        StatRecord::from_metadata(&std::fs::metadata(dir.as_str()).unwrap()),
        StatRecord::from_metadata(&std::fs::symlink_metadata(&link).unwrap()),
        StatRecord::from_metadata(&std::fs::metadata(&file).unwrap()).without_times(),
    ];
    for rec in records {
        let bytes = rec.encode();
        assert_eq!(StatRecord::decode(&bytes), Some(rec), "{rec:?}");
        for len in 0..bytes.len() {
            assert_eq!(
                StatRecord::decode(&bytes[..len]),
                None,
                "{len} of {} bytes",
                bytes.len()
            );
        }
        let mut longer = bytes.clone();
        longer.push(0);
        assert_eq!(StatRecord::decode(&longer), None, "one byte too many");
    }
}

/// What `statx(2)` fills for `path`, asked as std asks it, through a final
/// symlink when `follow` holds.
#[cfg(target_os = "linux")]
fn statx_of(path: &str, follow: bool) -> libc::statx {
    let c = std::ffi::CString::new(path).unwrap();
    let mut flags = libc::AT_STATX_SYNC_AS_STAT;
    if !follow {
        flags |= libc::AT_SYMLINK_NOFOLLOW;
    }
    // SAFETY: an all-zero statx is a valid value for the kernel to fill.
    let mut buf: libc::statx = unsafe { std::mem::zeroed() };
    let mask = libc::STATX_BASIC_STATS | libc::STATX_BTIME;
    let rc = unsafe { libc::statx(libc::AT_FDCWD, c.as_ptr(), flags, mask, &mut buf) };
    assert_eq!(rc, 0, "statx {path}");
    buf
}

/// The record the ring's stat builds from a `statx` is the record the pool's
/// builds from std's metadata, field for field, the times to the bit.
///
/// The counter-factual: a device number built from the major alone would answer
/// `file/stat` two ways for one path, by platform. So would a time that drops
/// its nanoseconds, or a birth time read where the kernel left none.
#[cfg(target_os = "linux")]
#[test]
fn a_statx_record_is_the_record_of_the_same_metadata() {
    let dir = ScratchDir::new("stat-statx");
    let file = dir.join_str("file");
    std::fs::write(&file, "hello").unwrap();
    let link = dir.join_str("link");
    std::os::unix::fs::symlink(&file, &link).unwrap();
    let fifo = dir.join_str("fifo");
    let c = std::ffi::CString::new(fifo.as_str()).unwrap();
    assert_eq!(unsafe { libc::mkfifo(c.as_ptr(), 0o600) }, 0);

    let cases = [
        (file.as_str(), true),
        (link.as_str(), true),
        (link.as_str(), false),
        (dir.as_str(), true),
        (fifo.as_str(), true),
    ];
    for (path, follow) in cases {
        let meta = if follow {
            std::fs::metadata(path)
        } else {
            std::fs::symlink_metadata(path)
        }
        .unwrap();
        let buf = statx_of(path, follow);
        assert_eq!(
            StatRecord::from_statx(&buf),
            StatRecord::from_metadata(&meta),
            "{path}, follow {follow}"
        );
    }

    let mut no_birth = statx_of(&file, true);
    no_birth.stx_mask &= !libc::STATX_BTIME;
    assert_eq!(
        StatRecord::from_statx(&no_birth).created,
        None,
        "a birth time the kernel did not report is absent"
    );
}

/// A file type outside the four the record names is not a record.
#[test]
fn an_unknown_file_type_does_not_decode() {
    let dir = ScratchDir::new("stat-kind");
    let rec = StatRecord::from_metadata(&std::fs::metadata(dir.as_str()).unwrap());
    let mut bytes = rec.encode();
    assert!(!bytes.is_empty(), "a record encodes to bytes");
    bytes[0] = 0xee;
    assert_eq!(StatRecord::decode(&bytes), None);
}
