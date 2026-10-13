// audited: 2026-10-20
// The warm cache: what a boot stores, under which name, what the next boot
// reads, and what a store leaves behind.
// docs/impl/image/boot.md
// docs/impl/image/plan.md

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

/// The executable tag the image this binary stores under `dir` opens with.
fn own_tag(dir: &std::path::Path) -> String {
    let path = policy_in(dir)
        .path()
        .expect("this binary names an image path");
    let name = path
        .file_name()
        .and_then(|n| n.to_str())
        .expect("a file name")
        .to_string();
    let (tag, _) = name
        .split_once('-')
        .unwrap_or_else(|| panic!("the image name {name} carries no executable tag"));
    tag.to_string()
}

// § Test plan, "Warm cache": a second instance over one directory boots from
// the image the first stored, and the stored file is the one the policy names.
// The assertion is on the reported boot source, because a cache that silently
// never hits still yields a working runtime.
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

// § Test plan, "Warm cache": a valid image filed under the plain source digest,
// which is the name an older build gave it, is never opened. The key holds the
// binary's identity, so an image another build stored has another name.
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

// § Test plan, "Warm cache": a store prunes its own executable's superseded
// file and leaves another executable's. Every rebuild orphans the last file,
// at megabytes apiece, and `elle`, `elle-rig` and each test binary share the
// default directory.
//
// The counter-factual for the second half is a store that prunes every other
// image: two executables sharing a directory then evict each other, and each
// start after a switch pays the whole front end.
#[test]
fn a_store_prunes_its_own_superseded_image_and_keeps_another_executables() {
    let dir = crate::common::ScratchDir::new("image-warm-prune");
    let tag = own_tag(dir.path());
    let other_tag = if tag == "0000000000000000" {
        "1111111111111111"
    } else {
        "0000000000000000"
    };
    std::fs::create_dir_all(dir.join("boot")).expect("mkdir");
    let superseded = dir
        .join("boot")
        .join(format!("{tag}-0123456789abcdef.image"));
    let anothers = dir
        .join("boot")
        .join(format!("{other_tag}-0123456789abcdef.image"));
    for planted in [&superseded, &anothers] {
        std::fs::write(planted, b"an image some other build stored").expect("plant an image");
    }

    drop(runtime_in(dir.path()));

    assert!(
        !superseded.exists(),
        "the store kept its own executable's superseded image"
    );
    assert!(
        anothers.exists(),
        "the store pruned another executable's image"
    );
    assert_eq!(
        images(dir.path()).len(),
        2,
        "the store left other files: {:?}",
        images(dir.path())
    );
}
