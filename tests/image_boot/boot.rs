// audited: 2026-09-21
// The boot configuration end to end: an image dumped from a booted instance,
// and a fresh instance that answers out of it.
// docs/impl/image/boot.md
// docs/impl/image/plan.md

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;
use elle::image::boot::{self, BootImage, BootSource};
use elle::pipeline::eval_all;
use elle::runtime::BootCaches;

/// A runtime whose boot image and compiled stdlib both live under `dir`, so
/// the only files it can hit are the ones this test wrote.
fn runtime_in(dir: &std::path::Path) -> Runtime {
    Runtime::with_caches(BootCaches {
        stdlib: StdlibCache::Dir(dir.join("stdlib")),
        image: BootImage::Dir(dir.join("boot")),
    })
}

/// Evaluate `source` in `rt` and answer its value.
fn eval(rt: &mut Runtime, source: &str) -> Value {
    let (vm, symbols, cctx) = rt.parts();
    eval_all(source, symbols, vm, cctx, "<image-boot>").expect("eval")
}

/// Leave a boot image in `dir`: one source boot, which stores the image every
/// later runtime over the same directory hydrates.
fn seed(dir: &std::path::Path) {
    let rt = runtime_in(dir);
    assert_eq!(
        rt.boot_source(),
        BootSource::Compiled,
        "the seeding runtime hydrated an image it was supposed to write"
    );
}

/// The image file the warm cache stores under `dir`.
fn stored_image(dir: &std::path::Path) -> std::path::PathBuf {
    let entries: Vec<std::path::PathBuf> = std::fs::read_dir(dir.join("boot"))
        .expect("the boot directory exists")
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|e| e == "image"))
        .collect();
    assert_eq!(entries.len(), 1, "expected one stored image: {entries:?}");
    entries.into_iter().next().expect("one entry")
}

/// One expression per layer of the boot state, with the answer a source boot
/// gives. `+` is a stdlib closure over the `%add` intrinsic rather than a
/// primitive, so it is a hydrated closure call; `last` is a core.lisp export;
/// `when` is a prelude macro, so it expands through a hydrated template.
const LAYERS: [(&str, i64); 4] = [
    ("(+ 1 2)", 3),
    ("(length [1 2 3])", 3),
    ("(last (list 1 2 3))", 3),
    ("(when true 42)", 42),
];

// § Test plan, "Boot": an instance that hydrated a boot image answers a stdlib
// call, a core call and a prelude macro expansion as a source-booted one does.
// It also reports that it booted from the image, because behaviour alone
// cannot tell the two apart — a warm cache that silently never hits yields a
// working runtime and would pass every assertion below.
#[test]
fn an_image_booted_instance_answers_as_a_source_booted_one() {
    let dir = crate::common::ScratchDir::new("image-boot-answers");
    seed(dir.path());

    let mut rt = runtime_in(dir.path());
    assert_eq!(
        rt.boot_source(),
        BootSource::Image,
        "this runtime must hydrate the image the seeding one stored"
    );
    for (source, want) in LAYERS {
        assert_eq!(
            eval(&mut rt, source).as_int(),
            Some(want),
            "{source} answered wrong after an image boot"
        );
    }
}

// § Test plan, "Boot": a fresh instance prints a hydrated stdlib closure's
// name and answers `meta/origin` with the file it was written in, both out of
// the image's tables.
//
// The counter-factual is the stdlib disk cache beside it, which drops the
// defining span outright (docs/impl/stdlib-cache.md): a cache-hit boot answers
// nil here. So this assertion distinguishes an image boot from the other
// skip-the-front-end path, and it exercises the file table at boot scale,
// where every span in 2900 lines of library names one file.
#[test]
fn a_hydrated_stdlib_closure_reports_where_it_was_written() {
    let dir = crate::common::ScratchDir::new("image-boot-origin");
    seed(dir.path());

    let mut rt = runtime_in(dir.path());
    assert_eq!(rt.boot_source(), BootSource::Image, "no image boot");
    let file = eval(&mut rt, "(get (meta/origin +) :file)");
    assert_eq!(
        file.as_str().map(str::to_owned),
        Some("<stdlib>".to_string()),
        "a hydrated stdlib closure lost its origin"
    );
    // The line and the column cross beside the file, so the whole span is
    // pinned rather than the one field the file table rewrites.
    for field in [":line", ":col"] {
        let answer = eval(&mut rt, &format!("(get (meta/origin +) {field})"));
        assert!(
            answer.as_int().is_some_and(|n| n > 0),
            "a hydrated origin answers {field} with {answer}"
        );
    }
}

// § Test plan, "Boot": a macro the boot never expanded still expands after
// hydration, which is the lazy transformer fill working over a hydrated
// template. `->` threads its argument through each form, so a wrong expansion
// answers a number rather than failing to compile.
#[test]
fn a_macro_expands_after_an_image_boot() {
    let dir = crate::common::ScratchDir::new("image-boot-macro");
    seed(dir.path());

    let mut rt = runtime_in(dir.path());
    assert_eq!(rt.boot_source(), BootSource::Image, "no image boot");
    assert_eq!(
        eval(&mut rt, "(-> 1 (+ 2) (* 3))").as_int(),
        Some(9),
        "a hydrated macro expanded wrong"
    );
    // A macro defined after the boot expands beside the hydrated ones, so the
    // expander is a working expander and not just a table of templates.
    assert_eq!(
        eval(&mut rt, "(defmacro twice [x] `(+ ,x ,x)) (twice 21)").as_int(),
        Some(42),
        "a macro defined after an image boot expanded wrong"
    );
}

// § Test plan, "Boot": an image whose source digest is not this binary's is
// refused by its own name, not the fingerprint's.
//
// The counter-factual is an instance that answers with the previous library:
// the layout such an image carries is one this binary can map, so nothing in
// the fingerprint, the verifier or the object walk can see that its stdlib is
// somebody else's.
#[test]
fn an_image_from_other_sources_is_refused_by_its_digest() {
    let dir = crate::common::ScratchDir::new("image-boot-digest");
    seed(dir.path());
    let path = stored_image(dir.path());

    // Flip one byte of the digest the image carries, which is what an image
    // built from edited sources would hold.
    let mut bytes = std::fs::read(&path).expect("read image");
    let digest = boot::sources_digest();
    assert!(!digest.is_empty(), "this binary reports no source digest");
    let at = bytes
        .windows(digest.len())
        .position(|w| w == digest.as_bytes())
        .expect("the image does not carry the source digest");
    bytes[at] ^= 0x20;
    std::fs::write(&path, &bytes).expect("rewrite image");

    let mut rt = Runtime::without_stdlib();
    let (heap, symbols) = rt.heap_and_symbols();
    match boot::hydrate_path(heap, symbols, &path) {
        Err(ImageError::Sources { .. }) => {}
        other => panic!("expected a digest refusal, got {other:?}"),
    }
}

// § Test plan, "Boot": the instance boots from source instead, rather than
// answering with the image's library. The refusal above is the diagnosis; this
// is the behaviour it buys.
#[test]
fn a_refused_image_falls_back_to_a_source_boot() {
    let dir = crate::common::ScratchDir::new("image-boot-fallback");
    seed(dir.path());
    let path = stored_image(dir.path());
    // Truncation past the header: mappable geometry, unreadable sections.
    let bytes = std::fs::read(&path).expect("read image");
    std::fs::write(&path, &bytes[..bytes.len() / 2]).expect("truncate image");

    let mut rt = runtime_in(dir.path());
    assert_eq!(
        rt.boot_source(),
        BootSource::Compiled,
        "a runtime hydrated a truncated image"
    );
    assert_eq!(
        eval(&mut rt, "(+ 1 2)").as_int(),
        Some(3),
        "the fallback boot does not work"
    );
}

// § Test plan, "Boot": two dumps of one boot state write one file. This is
// determinism at the scale where a code payload's padding can leak — 200
// closure templates and their constants, against the handful the small graphs
// carry (docs/impl/image/plan.md § Determinism owns the mechanism).
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

// The signal-bit refusal is in `tests/image_boot_signal.rs`, a binary of its
// own: the signal registry is process-global, so registering a bit here
// refuses every boot dump running beside it.
