// audited: 2026-10-21
// The warm cache: what a boot stores, under which name, what the next boot
// reads, and what a store leaves behind.
// docs/impl/image/boot.md
// docs/impl/image/pins.md

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;
use elle::image::boot::{self, BootImage, BootSource};
use elle::runtime::BootCaches;

/// The warm-cache policy a test over `dir` boots under.
fn policy_in(dir: &std::path::Path) -> BootImage {
    BootImage::Dir(dir.join("boot"))
}

/// A runtime whose boot image lives in `dir` and whose compiled stdlib is
/// cached beside it, so the only files it can hit are this test's.
fn runtime_in(dir: &std::path::Path) -> Runtime {
    Runtime::with_caches(BootCaches {
        stdlib: StdlibCache::Dir(dir.join("stdlib")),
        image: policy_in(dir),
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

// A second instance over one directory boots from the image the first stored,
// and the stored file is the one the policy names. The assertion is on the
// reported boot source, because a cache that silently never hits still yields
// a working runtime.
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
    assert_eq!(
        stored.len(),
        1,
        "the first boot stored no image: {stored:?}"
    );
    assert_eq!(
        Some(&stored[0]),
        policy_in(dir.path()).path().as_ref(),
        "the stored image is not the file the policy names"
    );

    let second = runtime_in(dir.path());
    assert_eq!(
        second.boot_source(),
        BootSource::Image,
        "the second runtime did not hydrate the stored image"
    );
}

// A valid image filed under the plain source digest, which is the name an older
// build gave it, is never opened. The key holds the binary's identity, so an
// image another build stored has another name.
//
// The counter-factual is the digest alone. The image below is valid and its
// digest matches, so a loader that keyed on the digest would hydrate it, and
// would hydrate an older build's code the same way.
#[test]
fn an_image_under_the_plain_digest_is_never_opened() {
    let seeded = crate::common::ScratchDir::new("image-warm-plain-seed");
    drop(runtime_in(seeded.path()));
    let valid = images(seeded.path()).into_iter().next().expect("one image");

    let dir = crate::common::ScratchDir::new("image-warm-plain");
    std::fs::create_dir_all(dir.join("boot")).expect("mkdir");
    let plain = dir
        .join("boot")
        .join(format!("{}.image", boot::sources_digest()));
    std::fs::copy(&valid, &plain).expect("file the image under the plain digest");

    let rt = runtime_in(dir.path());
    assert_eq!(
        rt.boot_source(),
        BootSource::Compiled,
        "a runtime hydrated an image filed under the plain digest"
    );
}

// A rejected file is replaced rather than rejected again — the start that
// meets it compiles and stores, and the start after that hits.
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

// A store prunes every other image in its directory, another executable's
// included. Every rebuild orphans the last file, at megabytes apiece, and a
// rustdoc test runs from a fresh temporary executable each time, so only a
// rule over the whole directory bounds it.
//
// The counter-factual is a store that keeps another executable's images: the
// directory then grows by one image for every executable that ever booted in
// it.
#[test]
fn a_store_prunes_every_other_image() {
    let dir = crate::common::ScratchDir::new("image-warm-prune");
    std::fs::create_dir_all(dir.join("boot")).expect("mkdir");
    let planted: Vec<std::path::PathBuf> = [
        "0123456789abcdef.image",
        "0000000000000000-0123456789abcdef.image",
    ]
    .iter()
    .map(|name| dir.join("boot").join(name))
    .collect();
    for path in &planted {
        std::fs::write(path, b"an image some other build stored").expect("plant an image");
    }

    drop(runtime_in(dir.path()));

    let kept = images(dir.path());
    assert_eq!(
        kept.len(),
        1,
        "the store did not prune every other image: {kept:?}"
    );
    assert_eq!(
        Some(&kept[0]),
        policy_in(dir.path()).path().as_ref(),
        "the image the store kept is not the one it wrote"
    );
}
