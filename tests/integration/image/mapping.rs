// audited: 2026-09-08
// How a hydrated region behaves as memory: the pool, teardown, and the
// kernel's rule for a file offset.
// docs/impl/image/plan.md

use super::*;
use elle::image;

// § Test plan, "Mapping": release a file-backed page and assert the pool
// unmapped it rather than caching it. Per-heap byte accounting is the
// race-free observable: `allocated_bytes` counts region pages AND cached
// pages, so if the freed hydrated pages were cached the count would not
// return to its pre-hydration value.
#[test]
fn hydrated_pages_release_by_munmap_not_cache() {
    let dir = crate::common::ScratchDir::new("image-munmap");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    dump_graph(&mut src, &path);

    let mut dst = FiberHeap::new();
    let before = dst.allocated_bytes();
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");
    assert!(
        dst.allocated_bytes() > before,
        "hydration added no page bytes"
    );
    dst.decref_region_if_present(hydrated.region);
    assert_eq!(
        dst.allocated_bytes(),
        before,
        "freed hydrated pages were cached instead of unmapped"
    );
}

// § Test plan, "Hygiene": hydrate, free, and the heap returns to its
// baseline — no image-specific carve-out in the region accounting.
#[test]
fn hydration_teardown_returns_to_baseline() {
    let dir = crate::common::ScratchDir::new("image-baseline");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    dump_graph(&mut src, &path);
    // The dump's scratch region was dropped: the source heap is back to its
    // own baseline (the graph's one region).
    assert_eq!(src.active_region_count(), 1, "dump leaked its scratch");

    let mut dst = FiberHeap::new();
    let regions_before = dst.region_info_vec();
    let objs_before = dst.visible_len();
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");
    assert!(dst.visible_len() > objs_before);
    dst.decref_region_if_present(hydrated.region);
    assert_eq!(dst.region_info_vec(), regions_before);
    assert_eq!(dst.visible_len(), objs_before);
}

// § Hydration: "Never rewrite an image file in place" — the atomic
// temp-and-rename discipline keeps a mapped old inode stable while a new
// file replaces the path. Pin the read side: replace the file by rename
// while a hydration is live, then read the hydrated values.
#[test]
fn replaced_file_keeps_live_mapping_intact() {
    let dir = crate::common::ScratchDir::new("image-rename");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let root = dump_graph(&mut src, &path);

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &path).expect("hydrate");

    // Replace the path with a different (garbage) file via rename.
    let replacement = dir.join("replacement");
    std::fs::write(&replacement, vec![0xAA; 1 << 16]).expect("write replacement");
    std::fs::rename(&replacement, &path).expect("rename over image");

    assert_eq!(
        root, hydrated.root,
        "live mapping read through the replaced path's old inode failed"
    );
}

// ── The platform rule the file geometry is built on ─────────────────

// The trap, and the reason the pages section is aligned at all: `mmap`
// refuses a file offset that is not a multiple of the OS page size. This
// cost six hydration failures on macOS arm64, where the page is 16 KiB and
// the pages section started at a hardcoded 4 KiB — legal on the 4 KiB hosts
// every other CI job runs, EINVAL there.
//
// The counter-factual this pins: the round-trip tests cannot catch that
// class of bug, because the dumper and the hydrator agreed on the wrong
// offset and agreement is all a round trip checks. This test asserts against
// the kernel instead of against the other half of our own code.
#[test]
fn mmap_refuses_a_file_offset_off_the_page_boundary() {
    let dir = crate::common::ScratchDir::new("image-mmap-offset");
    let path = dir.join("offsets.bin");

    let page = unsafe { libc::sysconf(libc::_SC_PAGESIZE) } as usize;
    std::fs::write(&path, vec![0u8; page * 4]).expect("write");
    let file = std::fs::File::open(&path).expect("open");
    let fd = std::os::fd::AsRawFd::as_raw_fd(&file);

    let map_at = |offset: usize| unsafe {
        libc::mmap(
            std::ptr::null_mut(),
            page,
            libc::PROT_READ,
            libc::MAP_PRIVATE,
            fd,
            offset as libc::off_t,
        )
    };

    // A page-aligned offset maps; half a page in does not.
    let good = map_at(page);
    assert_ne!(good, libc::MAP_FAILED, "page-aligned offset must map");
    unsafe { libc::munmap(good, page) };

    let bad = map_at(page / 2);
    assert_eq!(
        bad,
        libc::MAP_FAILED,
        "a half-page offset mapped; this platform's rule is not what the \
         image format assumes"
    );
    assert_eq!(
        std::io::Error::last_os_error().raw_os_error(),
        Some(libc::EINVAL),
        "expected EINVAL for a misaligned file offset"
    );
}
