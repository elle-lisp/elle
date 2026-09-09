// audited: 2026-09-09
// Where an image's bytes come from: a descriptor and an offset, never a path.
// docs/impl/image/format.md

use super::*;
use elle::image::{self, ImageError, ImageSource};

/// The OS base page — the unit every image offset must be a multiple of.
fn base_page() -> usize {
    unsafe { libc::sysconf(libc::_SC_PAGESIZE) as usize }
}

/// Write `image` into a larger file at `offset`, filling the gap. This is the
/// shape the embedded blob has: an image parked inside a bigger artifact.
fn park_image_at(path: &std::path::Path, offset: usize, image: &[u8]) {
    let mut bytes = vec![0x7Eu8; offset];
    bytes.extend_from_slice(image);
    std::fs::write(path, &bytes).expect("write container");
}

// § Test plan, "Source": an image parked at a non-zero, base-page-aligned
// offset inside a larger file hydrates from that descriptor and offset.
//
// The counter-factual is the second half: the same descriptor at offset 0 is
// not an image at all. A hydrator that ignored the offset would still find
// the magic — it reads a fixed distance from wherever it starts — so proving
// the offset is honored takes a container whose head is not an image.
#[test]
fn an_image_parked_at_an_offset_hydrates_from_its_descriptor() {
    let dir = crate::common::ScratchDir::new("image-parked");
    let plain = dir.join("graph.image");
    let container = dir.join("container.bin");

    let mut src = FiberHeap::new();
    let root = dump_graph(&mut src, &plain);
    let bytes = std::fs::read(&plain).expect("read image");
    let offset = base_page();
    park_image_at(&container, offset, &bytes);

    let file = std::fs::File::open(&container).expect("open container");
    let source = ImageSource::at(file, offset as u64).expect("aligned offset");
    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &mut SymbolTable::new(), &source).expect("hydrate at offset");
    assert_eq!(
        root, hydrated.root,
        "the graph read back from a parked image differs from the source"
    );

    let head = std::fs::File::open(&container).expect("reopen container");
    let at_zero = ImageSource::at(head, 0).expect("zero is aligned");
    match image::hydrate(&mut dst, &mut SymbolTable::new(), &at_zero) {
        Err(ImageError::Corrupt(_)) => {}
        other => panic!("offset 0 of the container is not an image, got {other:?}"),
    }
}

// § "An image starts at a base-page boundary of its container": every page
// maps at the image's offset plus a base-page multiple, so `mmap` accepts the
// sum only when the image's own offset is a multiple of the base page. The
// refusal is named, and it happens before anything is mapped.
//
// The trap this guards: an 8-byte misalignment is invisible until a page
// maps, and it fails there as a bare EINVAL from the kernel with nothing
// saying which offset was wrong.
#[test]
fn a_misaligned_offset_is_refused_by_name() {
    let dir = crate::common::ScratchDir::new("image-misaligned");
    let plain = dir.join("graph.image");
    let container = dir.join("container.bin");

    let mut src = FiberHeap::new();
    dump_graph(&mut src, &plain);
    let bytes = std::fs::read(&plain).expect("read image");
    let offset = base_page() + 8;
    park_image_at(&container, offset, &bytes);

    let file = std::fs::File::open(&container).expect("open container");
    match ImageSource::at(file, offset as u64) {
        Err(ImageError::Unaligned { offset: got, page }) => {
            assert_eq!(got, offset as u64, "the refusal names the wrong offset");
            assert_eq!(page, base_page(), "the refusal names the wrong page size");
        }
        other => panic!("expected an alignment refusal, got {other:?}"),
    }
}

// § Test plan, "Bytes": an image that arrives as bytes hydrates through an
// anonymous memory file, with no filesystem path anywhere in the path. The
// file the bytes came from is deleted first, so a hydrator that reopened a
// path could not pass this.
#[test]
fn bytes_hydrate_through_an_anonymous_file() {
    let dir = crate::common::ScratchDir::new("image-bytes");
    let plain = dir.join("graph.image");

    let mut src = FiberHeap::new();
    let root = dump_graph(&mut src, &plain);
    let bytes = std::fs::read(&plain).expect("read image");
    std::fs::remove_file(&plain).expect("remove the on-disk image");

    let source = ImageSource::from_bytes(&bytes).expect("anonymous file");
    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &mut SymbolTable::new(), &source).expect("hydrate from bytes");
    assert_eq!(root, hydrated.root, "the graph read back from bytes differs");
    assert!(!plain.exists(), "the test's own precondition went stale");
}

// § "Only one of the two anonymous files is a file": a read from the
// descriptor goes through ImageSource, because a Darwin shared-memory object
// refuses `pread` and only `mmap` reaches its bytes.
//
// The trap: a mapping starts at a base-page boundary and a read does not, so
// the non-`pread` arm maps from the page below the offset and copies out of
// the middle of it. An arm that mapped from the offset itself fails with
// EINVAL, and one that dropped the slack returns the wrong bytes silently.
// This is the only test that reaches that arithmetic — every other read in the
// suite starts at an offset a page already begins on.
#[test]
fn a_read_from_a_source_starts_where_it_was_asked_to() {
    let dir = crate::common::ScratchDir::new("image-read-at");
    let container = dir.join("container.bin");

    let bytes: Vec<u8> = (0..4u32 * base_page() as u32)
        .map(|i| (i % 251) as u8)
        .collect();
    std::fs::write(&container, &bytes).expect("write container");
    let file = std::fs::File::open(&container).expect("open container");
    let source = ImageSource::at(file, base_page() as u64).expect("aligned offset");

    // Deliberately misaligned, and long enough to leave the page it starts in.
    let at = base_page() as u64 + 37;
    let mut got = vec![0u8; base_page() + 11];
    source.read_exact_at(&mut got, at).expect("read at an offset");
    assert_eq!(
        got.as_slice(),
        &bytes[at as usize..at as usize + got.len()],
        "the read returned bytes from the wrong place in the descriptor"
    );
}

// § Hydration: where the kernel mints memory files the anonymous file is
// write-sealed before it is mapped, so the immutability `MAP_PRIVATE` relies
// on is enforced by the kernel rather than by our own discipline. A write to
// that descriptor must fail.
//
// The cfg follows the call, not the name: Android's `target_os` is `"android"`
// and its kernel seals a memfd like any other Linux one, so a `linux`-only gate
// would skip the platform on the strength of its spelling.
#[cfg(any(target_os = "linux", target_os = "android"))]
#[test]
fn an_anonymous_image_file_refuses_writes() {
    use std::os::fd::AsRawFd;

    let dir = crate::common::ScratchDir::new("image-sealed");
    let plain = dir.join("graph.image");
    let mut src = FiberHeap::new();
    dump_graph(&mut src, &plain);
    let bytes = std::fs::read(&plain).expect("read image");

    let source = ImageSource::from_bytes(&bytes).expect("anonymous file");
    let poison = [0xFFu8; 8];
    let wrote = unsafe {
        libc::pwrite(
            source.as_fd().as_raw_fd(),
            poison.as_ptr() as *const libc::c_void,
            poison.len(),
            0,
        )
    };
    assert_eq!(
        wrote, -1,
        "the anonymous image file accepted a write; the seal is missing"
    );

    // The mapping still reads the bytes the source was built from.
    let mut dst = FiberHeap::new();
    image::hydrate(&mut dst, &mut SymbolTable::new(), &source).expect("hydrate after the refused write");
}
