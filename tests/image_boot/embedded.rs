// audited: 2026-10-21
// The embedded blob: an instance hydrates the blob it is given, stores
// nothing, and falls back to the warm cache when the blob is refused.
// docs/impl/image/blob.md
// docs/impl/image/boot.md
// docs/impl/image/pins.md
//
// Each blob here lives on the heap, so a start copies it into an anonymous
// file. src/image/boot/blob/tests.rs pins the mapping from the executable, and
// a release `make elle` checks the blob its binary carries.

use super::*;
use elle::compiler::stdlib_cache::StdlibCache;
use elle::image::boot::{Blob, BootImage, BootSource};
use elle::runtime::BootCaches;

/// `bytes`, copied into memory that lives for the rest of the process at the
/// alignment the embedded blob has.
fn aligned(bytes: &[u8]) -> &'static [u8] {
    let layout = std::alloc::Layout::from_size_align(bytes.len().max(1), 65536)
        .expect("a 64 KiB-aligned layout");
    // SAFETY: the layout has a non-zero size; the allocation is filled before
    // it is read, and never freed, so the slice is valid for 'static.
    unsafe {
        let ptr = std::alloc::alloc(layout);
        assert!(!ptr.is_null(), "allocate the blob");
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), ptr, bytes.len());
        std::slice::from_raw_parts(ptr, bytes.len())
    }
}

/// A valid boot image of this binary's sources, as a blob.
fn valid_blob() -> Blob {
    let dir = crate::common::ScratchDir::new("image-blob-dump");
    let path = dir.join("boot.image");
    let mut rt = Runtime::with_stdlib_cache(StdlibCache::Off);
    rt.dump_boot_image(&path).expect("dump a boot image");
    drop(rt);
    Blob::new(aligned(&std::fs::read(&path).expect("read the dump")))
}

/// A blob every start refuses.
fn refused_blob() -> Blob {
    Blob::new(aligned(&[0xa5; 8192]))
}

/// A runtime that boots from `blob`, falling back to the warm cache in
/// `fallback`, with a stdlib cache under `dir` so the only files it can hit
/// are this test's.
fn runtime_over(
    dir: &std::path::Path,
    blob: Blob,
    fallback: Option<std::path::PathBuf>,
) -> Runtime {
    Runtime::with_caches(BootCaches {
        stdlib: StdlibCache::Dir(dir.join("stdlib")),
        image: BootImage::Embedded { blob, fallback },
    })
}

/// Every `.image` file in `dir`, which may not exist.
fn images_in(dir: &std::path::Path) -> Vec<std::path::PathBuf> {
    std::fs::read_dir(dir)
        .map(|entries| {
            entries
                .flatten()
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|e| e == "image"))
                .collect()
        })
        .unwrap_or_default()
}

// An instance hydrates a valid blob and stores nothing, because the image a
// store would write is the one the blob holds.
//
// The counter-factual is a boot that ignores the blob: it compiles, answers
// every call, and stores an image in the warm cache.
#[test]
fn an_instance_hydrates_a_valid_blob_and_stores_nothing() {
    let dir = crate::common::ScratchDir::new("image-blob-hit");
    let warm = dir.join("boot");
    let mut rt = runtime_over(dir.path(), valid_blob(), Some(warm.clone()));
    assert_eq!(
        rt.boot_source(),
        BootSource::Image,
        "the runtime did not hydrate the blob it was given"
    );
    let (vm, symbols, cctx) = rt.parts();
    let three = elle::pipeline::eval_all("(+ 1 2)", symbols, vm, cctx, "<blob>").expect("eval");
    assert_eq!(
        three.as_int(),
        Some(3),
        "the hydrated stdlib answered wrongly"
    );
    drop(rt);
    assert!(
        images_in(&warm).is_empty(),
        "a runtime that hydrated its blob stored an image: {:?}",
        images_in(&warm)
    );
}

// A refused blob falls back to the warm cache, and a stored image there
// hydrates. The counter-factual is a boot that stops at the blob, which
// compiles the three sources every time a host's page size refuses the blob.
#[test]
fn a_refused_blob_falls_back_to_the_warm_cache() {
    let dir = crate::common::ScratchDir::new("image-blob-fallback");
    let warm = dir.join("boot");
    let seeding = Runtime::with_caches(BootCaches {
        stdlib: StdlibCache::Dir(dir.join("stdlib")),
        image: BootImage::Dir(warm.clone()),
    });
    assert_eq!(seeding.boot_source(), BootSource::Compiled);
    drop(seeding);

    let rt = runtime_over(dir.path(), refused_blob(), Some(warm));
    assert_eq!(
        rt.boot_source(),
        BootSource::Image,
        "a refused blob did not fall back to the image the warm cache holds"
    );
}

// A refused blob over a cold warm cache compiles and stores, so the next start
// hydrates the stored image rather than compiling again.
#[test]
fn a_refused_blob_over_a_cold_cache_compiles_and_stores() {
    let dir = crate::common::ScratchDir::new("image-blob-cold");
    let warm = dir.join("boot");
    let first = runtime_over(dir.path(), refused_blob(), Some(warm.clone()));
    assert_eq!(first.boot_source(), BootSource::Compiled);
    drop(first);
    assert_eq!(
        images_in(&warm).len(),
        1,
        "a refused blob over a cold cache stored no image"
    );

    let next = runtime_over(dir.path(), refused_blob(), Some(warm));
    assert_eq!(
        next.boot_source(),
        BootSource::Image,
        "the start after the store did not hydrate it"
    );
}

// A refused blob with no warm cache compiles, and writes no image anywhere.
#[test]
fn a_refused_blob_with_no_fallback_compiles() {
    let dir = crate::common::ScratchDir::new("image-blob-alone");
    let rt = runtime_over(dir.path(), refused_blob(), None);
    assert_eq!(rt.boot_source(), BootSource::Compiled);
    drop(rt);
    assert!(
        !dir.join("boot").exists(),
        "a blob with no fallback wrote a warm cache"
    );
}
