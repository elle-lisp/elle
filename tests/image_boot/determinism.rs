// audited: 2026-10-21
// A boot dump is deterministic: two dumps of one boot state, and two dumps
// with a compile between them, each write one file.
// docs/impl/image/boot.md
// docs/impl/image/pins.md

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;

// Two dumps of one boot state write one file. This is determinism at the scale
// where a code payload's padding can leak: every closure template of the
// library and its constants, against the handful the small graphs carry
// (docs/impl/image.md owns the mechanism).
#[test]
fn two_dumps_of_one_boot_state_write_one_file() {
    let dir = crate::common::ScratchDir::new("image-boot-determinism");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    // The stdlib is compiled rather than read from its disk cache: a cache hit
    // rebuilds the library's closures through the send codec, whose capture
    // cells record no binding, and the dump refuses one of those
    // (docs/impl/image/boot.md). With `Runtime::new()` here the verdict would
    // move with the state of a file no assertion below names.
    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Off);
    paint_stack(0xAA, 16);
    rt.dump_boot_image(&a).expect("dump a");
    paint_stack(0x55, 16);
    rt.dump_boot_image(&b).expect("dump b");

    let (ba, bb) = (
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
    );
    assert_eq!(ba.len(), bb.len(), "two boot dumps differ in length");
    let diff = (0..ba.len()).filter(|&i| ba[i] != bb[i]).count();
    assert_eq!(diff, 0, "two boot dumps differ at {diff} offsets");
}

// A boot image's static-region watermark is the mark its boot left, so a
// compile between two boot dumps changes neither file. The program below
// defines nothing, so the boot state the two dumps read is one state.
//
// The counter-factual is the live counter. The compile mints slots for its
// closure and its array, so a dump that read the counter as it ran would write
// a second file differing at the watermark alone.
#[test]
fn a_compile_between_two_boot_dumps_changes_neither_file() {
    let dir = crate::common::ScratchDir::new("image-boot-mark");
    let a = dir.join("a.image");
    let b = dir.join("b.image");

    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Off);
    rt.dump_boot_image(&a).expect("dump a");
    let answer = eval(&mut rt, "(length (map (fn [x] (+ x 1)) [1 2 3]))");
    assert_eq!(
        answer.as_int(),
        Some(3),
        "the compile between the dumps failed"
    );
    rt.dump_boot_image(&b).expect("dump b");

    assert!(
        std::fs::read(&a).expect("read a") == std::fs::read(&b).expect("read b"),
        "a compile between two boot dumps changed the file"
    );
}
