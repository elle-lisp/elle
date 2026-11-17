// audited: 2026-11-17
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
