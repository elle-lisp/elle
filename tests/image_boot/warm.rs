// audited: 2026-09-21
// The warm cache: what a boot stores, what the next boot reads, and what a
// store leaves behind.
// docs/impl/image/boot.md
// docs/impl/image/plan.md

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;
use elle::image::boot::{self, BootImage, BootSource};
use elle::runtime::BootCaches;

/// A runtime whose boot image lives in `dir` and whose compiled stdlib is
/// cached beside it, so the only files it can hit are this test's.
fn runtime_in(dir: &std::path::Path) -> Runtime {
    Runtime::with_caches(BootCaches {
        stdlib: StdlibCache::Dir(dir.join("stdlib")),
        image: BootImage::Dir(dir.join("boot")),
    })
}

/// Every `.image` file in `dir`'s boot directory, sorted.
fn images(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    let mut found: Vec<std::path::PathBuf> = std::fs::read_dir(dir.join("boot"))
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "image"))
                .collect()
        })
        .unwrap_or_default();
    found.sort();
    found
}

// § Test plan, "Warm cache": a second instance over one directory boots from
// the image the first stored. The assertion is on the reported boot source,
// because a cache that silently never hits still yields a working runtime.
#[test]
fn a_second_instance_boots_from_the_image_the_first_stored() {
    let dir = crate::common::ScratchDir::new("image-warm-hit");
    let first = runtime_in(dir.path());
    assert_eq!(
        first.boot_source(),
        BootSource::Compiled,
        "the first runtime met an empty directory and must have compiled"
    );
    drop(first);

    let stored = images(dir.path());
    assert_eq!(stored.len(), 1, "the first boot stored no image: {stored:?}");
    assert_eq!(
        stored[0].file_name().and_then(|n| n.to_str()),
        Some(format!("{}.image", boot::sources_digest()).as_str()),
        "the stored image is not named for the source digest"
    );

    let second = runtime_in(dir.path());
    assert_eq!(
        second.boot_source(),
        BootSource::Image,
        "the second runtime did not hydrate the stored image"
    );
}

// § Test plan, "Warm cache": a rejected file is replaced rather than rejected
// again — the start that meets it compiles and stores, and the start after
// that hits.
//
// The counter-factual is a loader that leaves a rejected file where it is:
// every later start pays the whole front end, and the directory holds a file
// nothing will ever read.
#[test]
fn a_rejected_image_is_replaced_rather_than_rejected_again() {
    let dir = crate::common::ScratchDir::new("image-warm-replace");
    drop(runtime_in(dir.path()));
    let path = images(dir.path()).into_iter().next().expect("one image");
    std::fs::write(&path, b"not an image at all").expect("clobber the image");

    let next = runtime_in(dir.path());
    assert_eq!(
        next.boot_source(),
        BootSource::Compiled,
        "a runtime hydrated a clobbered image"
    );
    drop(next);

    let after = runtime_in(dir.path());
    assert_eq!(
        after.boot_source(),
        BootSource::Image,
        "the clobbered file was rejected again instead of replaced"
    );
}

// § Test plan, "Warm cache": a store prunes the superseded file and leaves the
// kept one. Every edit to a boot source mints a new digest and orphans the
// last file, at megabytes apiece.
#[test]
fn a_store_prunes_the_superseded_image() {
    let dir = crate::common::ScratchDir::new("image-warm-prune");
    let orphan = dir.join("boot").join("0123456789abcdef.image");
    std::fs::create_dir_all(dir.join("boot")).expect("mkdir");
    std::fs::write(&orphan, b"an earlier digest's image").expect("write orphan");

    drop(runtime_in(dir.path()));

    let kept = images(dir.path());
    assert_eq!(
        kept.len(),
        1,
        "the store did not prune the superseded image: {kept:?}"
    );
    assert!(!orphan.exists(), "the orphan survived the store");
}

// § Test plan, "Warm cache": the default policy neither reads a directory nor
// writes one. The default is off while a hydrated stdlib reaches neither the
// JIT tier nor cross-unit inlining (docs/impl/image/boot.md), so a runtime
// that stored one anyway would turn the policy into a suggestion.
#[test]
fn the_default_policy_neither_reads_an_image_nor_writes_one() {
    let dir = crate::common::ScratchDir::new("image-warm-off");
    // Seed a valid image, so reading one is possible and only the policy
    // stops it.
    drop(runtime_in(dir.path()));
    let seeded = images(dir.path());
    assert_eq!(seeded.len(), 1, "no image to ignore");

    let rt = Runtime::with_caches(BootCaches {
        stdlib: StdlibCache::Dir(dir.join("stdlib")),
        image: BootImage::default(),
    });
    assert_eq!(
        rt.boot_source(),
        BootSource::Compiled,
        "the default policy hydrated an image"
    );
    drop(rt);
    assert_eq!(
        images(dir.path()),
        seeded,
        "the default policy wrote into the boot directory"
    );
}
