// audited: 2026-10-21
//! The warm cache's file name: one per source digest and build, and none for a binary that cannot identify itself.
//!
//! docs/impl/image/boot.md

use std::path::PathBuf;

use super::{sources_digest, BootImage, Executable};

// docs/impl/image/boot.md: one executable rebuilt gives one source digest
// two names. The counter-factual is a name from the digest alone: the new
// build opens the image the old one stored, and runs the code the old build
// compiled.
#[test]
fn two_builds_name_one_digests_image_apart() {
    let old = Executable::new(12_563_264, 1);
    let new = Executable::new(12_563_264, 2);
    let digest = sources_digest();
    assert_ne!(
        old.image_name(&digest),
        new.image_name(&digest),
        "two builds file one digest's image under one name"
    );
}

// docs/impl/image/boot.md: a binary that cannot read its own identity reads
// and writes no image, as the stdlib cache declines to cache. Falling back to
// the digest alone would bring back the confusion the identity exists to
// prevent.
#[test]
fn a_binary_with_no_identity_has_no_image_path() {
    let policy = BootImage::Dir(PathBuf::from("/nowhere/boot"));
    assert_eq!(policy.path_for(None), None);
}

/// The identity is read from the executable this process runs, which is the
/// file whose rebuild must change the key.
///
/// Characterization, not a failing-first regression: "a different build
/// yields a different key" is observable only across builds.
#[test]
fn the_build_identity_is_read_from_the_running_executable() {
    let exe = Executable::current().expect("the test binary can identify itself");
    let (len, _mtime) = exe.identity();
    let meta = std::fs::metadata(std::env::current_exe().expect("current_exe"))
        .expect("the executable's metadata");
    assert_eq!(
        len,
        meta.len(),
        "the identity must be the binary's own size"
    );
    assert!(len > 0, "a zero-length identity separates nothing");
}
