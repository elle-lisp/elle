// audited: 2026-10-21
// The damage-and-hydrate harness: dump one value, damage its bytes, and
// answer the refusal a fresh heap gives them.
// docs/impl/image/plan.md

use super::*;

/// Dump one lone value and answer its bytes and section ranges. The graph is
/// the caller's, so a test can put a single object in the image and know
/// exactly where its inline data sits.
pub(super) fn dumped_value(
    dir: &crate::common::ScratchDir,
    build: impl FnOnce(&mut FiberHeap, RuntimeRegion) -> Value,
) -> (Vec<u8>, Sections) {
    let path = dir.join("lone.image");
    let mut src = traited_heap();
    let region = src.new_runtime_region();
    let root = build(&mut src, region);
    image::dump(&mut src, &graph_names(), root, &path).expect("dump");
    let bytes = std::fs::read(&path).expect("read image");
    let sections = image::sections(&bytes).expect("a freshly dumped image parses");
    (bytes, sections)
}

/// Hydrate `bytes` in a fresh heap and answer the refusal, asserting the
/// failed load left neither a region nor page bytes behind.
pub(super) fn refusal(bytes: &[u8]) -> ImageError {
    let source = image::ImageSource::from_bytes(bytes).expect("anonymous file");
    let mut dst = traited_heap();
    let regions_before = dst.active_region_count();
    let bytes_before = dst.allocated_bytes();
    let err = match image::hydrate(&mut dst, &mut SymbolTable::new(), &source) {
        Err(e) => e,
        Ok(_) => panic!("the damaged image hydrated instead of being refused"),
    };
    assert_eq!(
        dst.active_region_count(),
        regions_before,
        "a refused hydration minted a region"
    );
    assert_eq!(
        dst.allocated_bytes(),
        bytes_before,
        "a refused hydration left pages mapped into the heap"
    );
    err
}

pub(super) fn put_u64(bytes: &mut [u8], at: usize, v: u64) {
    bytes[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

pub(super) fn get_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}
