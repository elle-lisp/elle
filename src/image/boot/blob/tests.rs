// audited: 2026-10-21
// Where a start finds a blob: in the file that holds it, at an aligned offset,
// or nowhere, and what it hydrates in each case.
// docs/impl/image/blob.md
// docs/impl/image/boot.md
// docs/impl/image/pins.md

use std::fs::File;
use std::os::unix::fs::{FileExt, MetadataExt};

use super::{holds, locate, Aligned, Blob};
use crate::image::boot::BootImage;

/// Bytes no two offsets share a run of, so a read from the wrong offset
/// differs from the probe in its first bytes.
const fn pattern<const N: usize>() -> [u8; N] {
    let mut out = [0u8; N];
    let mut i = 0;
    while i < N {
        out[i] = (i.wrapping_mul(31) ^ (i >> 8).wrapping_mul(7) ^ (i >> 16)) as u8;
        i += 1;
    }
    out
}

/// Two 64 KiB pages of pattern behind the wrapper the embedded blob sits
/// behind, so the probe is placed the way the blob is.
static PROBE: &Aligned<[u8]> = &Aligned(pattern::<131072>());

/// The file this process is running, by the name that names it whatever
/// replaced its path.
fn executable() -> std::fs::Metadata {
    #[cfg(any(target_os = "linux", target_os = "android"))]
    let path = std::path::PathBuf::from("/proc/self/exe");
    #[cfg(not(any(target_os = "linux", target_os = "android")))]
    let path = std::env::current_exe().expect("the running executable");
    std::fs::metadata(path).expect("measure the running executable")
}

/// Whether `file` is the executable this process runs.
fn is_executable(file: &File) -> bool {
    let exe = executable();
    let meta = file.metadata().expect("measure the file");
    meta.dev() == exe.dev() && meta.ino() == exe.ino()
}

/// `len` bytes of `file` from `at`.
fn read(file: &File, at: u64, len: usize) -> Vec<u8> {
    let mut buf = vec![0u8; len];
    file.read_exact_at(&mut buf, at).expect("read the file");
    buf
}

/// Bytes on the heap for the rest of the process, which no file holds.
fn leaked(bytes: &[u8]) -> &'static [u8] {
    Box::leak(bytes.to_vec().into_boxed_slice())
}

// A static behind the blob's wrapper sits in the executable at a 64 KiB-aligned
// offset, and the bytes there are the static's.
//
// The trap: a 64 KiB-aligned address does not make a 64 KiB-aligned file
// offset by itself. Load-segment congruence does, and it holds only for the
// offset the segment table gives; an offset taken from the page the address
// falls on would read the right bytes and fail every mapping.
#[test]
fn a_static_is_found_in_the_executable_at_an_aligned_offset() {
    let bytes = &PROBE.0;
    let located = locate(bytes).expect("the probe was not found in any file");
    assert!(
        is_executable(&located.file),
        "the probe was found in a file other than the running executable"
    );
    assert_eq!(
        located.offset % 65536,
        0,
        "the probe sits at file offset {:#x}, which no 64 KiB host can map",
        located.offset
    );
    assert!(
        read(&located.file, located.offset, bytes.len()) == bytes,
        "the file holds other bytes at offset {:#x}",
        located.offset
    );
}

// Bytes on the heap lie in no segment of any file, so no file holds them. The
// counter-factual is a search that answers the nearest segment, which a start
// would then map as if it held the blob.
#[test]
fn bytes_on_the_heap_are_found_in_no_file() {
    let bytes = leaked(&PROBE.0);
    assert!(
        locate(bytes).is_none(),
        "bytes on the heap were found in a file"
    );
}

// A file holds a blob at an offset when the bytes there begin with the blob's
// header block. A byte that differs inside the block is another image; a byte
// that differs past it is not looked at.
#[test]
fn a_file_holds_a_blob_only_when_its_header_block_matches() {
    let header = crate::image::format::HEADER_BLOCK;
    let blob = pattern::<8192>();
    let at = 4096u64;
    let file_with = |bytes: &[u8]| {
        let file = tempfile::tempfile().expect("a temporary file");
        file.write_all_at(bytes, at).expect("write the bytes");
        file
    };

    assert!(
        holds(&file_with(&blob), at, &blob),
        "a file whose bytes are the blob's does not hold it"
    );

    let mut inside = blob;
    inside[header - 1] ^= 0xff;
    assert!(
        !holds(&file_with(&inside), at, &blob),
        "a file whose header block differs in its last byte holds the blob"
    );

    let mut past = blob;
    past[header] ^= 0xff;
    assert!(
        holds(&file_with(&past), at, &blob),
        "a file whose bytes differ only past the header block does not hold the blob"
    );

    assert!(
        !holds(&file_with(&blob[..header / 2]), at, &blob),
        "a file that ends inside the header block holds the blob"
    );
}

// A blob the executable holds is mapped from the executable, at the offset the
// segment table gives. The counter-factual is a start that copies every blob,
// which hydrates and answers every call while every process pays a private
// copy of megabytes.
#[test]
fn a_blob_the_executable_holds_is_mapped_from_it() {
    let bytes = &PROBE.0;
    let open = Blob::new(bytes).open().expect("open the blob");
    assert!(
        is_executable(open.source().file()),
        "the blob was copied rather than mapped from the executable"
    );
    let located = locate(bytes).expect("the probe was not found in any file");
    assert_eq!(
        open.source().offset(),
        located.offset,
        "the blob maps from another offset than the one that holds it"
    );
}

// A blob no file holds is copied into an anonymous file, and the copy holds its
// bytes from offset zero.
#[test]
fn a_blob_no_file_holds_is_copied() {
    let bytes = leaked(&PROBE.0);
    let open = Blob::new(bytes).open().expect("open the blob");
    let source = open.source();
    assert!(
        !is_executable(source.file()),
        "a blob on the heap was mapped from the executable"
    );
    assert_eq!(source.offset(), 0, "a copy starts its file");
    assert!(
        read(source.file(), 0, bytes.len()) == bytes,
        "the copy does not hold the blob's bytes"
    );
}

// `Embedded` reads its blob, then its warm-cache directory, and the default
// reads the blob this binary carries.
#[test]
fn each_policy_names_the_blob_and_the_warm_cache_it_reads() {
    let blob = Blob::new(leaked(b"a blob"));
    let dir = std::path::PathBuf::from("/srv/boot");

    let embedded = BootImage::Embedded {
        blob,
        fallback: Some(dir.clone()),
    };
    assert_eq!(
        embedded.blob(),
        Some(blob),
        "Embedded does not read its blob"
    );
    let file = embedded
        .path()
        .expect("Embedded with a fallback names no warm-cache file");
    assert_eq!(
        file.parent(),
        Some(dir.as_path()),
        "Embedded does not fall back to its directory"
    );

    let alone = BootImage::Embedded {
        blob,
        fallback: None,
    };
    assert_eq!(
        alone.path(),
        None,
        "a blob with no fallback names a warm cache"
    );

    assert_eq!(BootImage::Process.blob(), Blob::this_binary());
    assert_eq!(BootImage::Dir(dir).blob(), None, "DIR reads a blob");
    assert_eq!(BootImage::Off.blob(), None, "off reads a blob");
}
