// audited: 2026-09-14
// What the verifier refuses in a closure header: the words and slots whose
// damage a range check cannot see.
// docs/impl/image/sealing.md

use super::*;
use elle::value::closure::materialize;
use elle::value::{Arity, HeapTag};

/// A lone closure over a five-byte bytecode: the smallest image carrying a
/// header, its payload, and the relocations the tests below damage.
fn dumped_closure(dir: &crate::common::ScratchDir) -> (Vec<u8>, Sections) {
    dumped_value(dir, |heap, region| {
        let proto = std::rc::Rc::new(TemplateProto::new(
            vec![7, 1, 4, 1, 9],
            Arity::Exact(0),
            Vec::new(),
        ));
        let template = TemplateRef::region(materialize(heap, &proto, region));
        let env = heap.alloc_region_slice_in_region::<Value>(&[], region);
        heap.alloc_in_region(
            HeapObject::Closure {
                closure: Closure::new(template, env, SignalBits::EMPTY),
                traits: Value::NIL,
            },
            region,
        )
    })
}

/// Where [`dumped_closure`]'s header keeps the fields the tests damage, in
/// pages-relative offsets — discovered rather than assumed. The header's one
/// relocation slot inside the shell is the payload slice's `ptr`, the word
/// after it holds the length (one), and the remaining word of the 24-byte
/// variant is the blueprint.
struct HeaderGeometry {
    /// The payload slice's `ptr` slot; its length word is 8 bytes further.
    slot: usize,
    /// The payload struct the slot targets.
    target: usize,
    /// File offset of the relocation entry naming the slot.
    reloc_entry: usize,
    /// The header's blueprint word.
    proto_at: usize,
}

fn header_geometry(bytes: &[u8], s: &Sections) -> HeaderGeometry {
    let shell = s
        .index
        .clone()
        .step_by(Sections::INDEX_BYTES)
        .find(|&entry| get_u64(bytes, entry + 8) == HeapTag::ClosureTemplate as u64)
        .map(|entry| get_u64(bytes, entry) as usize)
        .expect("no header in the index");
    // A probed variant's payload sits at offset 8 of the shell
    // (docs/impl/image/measurements.md item 6), so the slice starts at one of
    // the struct's first two words.
    let (reloc_entry, slot, target) = s
        .relocations
        .clone()
        .step_by(Sections::RELOC_BYTES)
        .map(|e| (e, get_u64(bytes, e) as usize, get_u64(bytes, e + 8) as usize))
        .find(|&(_, slot, _)| (shell + 8..shell + 32).contains(&slot))
        .expect("the header has a relocation slot");
    assert!(
        slot == shell + 8 || slot == shell + 16,
        "the payload slice is not at a word the 24-byte header can hold"
    );
    assert_eq!(
        get_u64(bytes, s.pages.start + slot + 8),
        1,
        "the word after the relocated ptr is not the length one, so this \
         discovery found some other field"
    );
    let proto_at = if slot == shell + 8 { shell + 24 } else { shell + 8 };
    assert_eq!(
        get_u64(bytes, s.pages.start + proto_at),
        0,
        "the blueprint word is not zero where the discovery placed it"
    );
    HeaderGeometry {
        slot,
        target,
        reloc_entry,
        proto_at,
    }
}

// A header's blueprint is a Rust-heap `Rc` no image writes, and the one bit
// pattern a teardown could hurt on. The verifier refuses the word while it is
// still only a word being read — before anything could drop a fabricated `Rc`
// (docs/impl/image/sealing.md § "A closure crosses without its blueprint").
#[test]
fn a_header_with_a_blueprint_word_is_refused() {
    let dir = crate::common::ScratchDir::new("image-header-blueprint");
    let (mut bytes, s) = dumped_closure(&dir);
    let g = header_geometry(&bytes, &s);
    put_u64(&mut bytes, s.pages.start + g.proto_at, 0x10);
    match refusal(&bytes) {
        ImageError::Corrupt(what) => assert!(
            what.contains("blueprint"),
            "the refusal does not name the blueprint: {what}"
        ),
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A header names exactly one payload. Zero is the length no range check can
// object to — no extent, nothing out of bounds — so only the count check
// stands between the file and a header whose payload read is undefined.
#[test]
fn a_header_naming_zero_payloads_is_refused() {
    let dir = crate::common::ScratchDir::new("image-header-count");
    let (mut bytes, s) = dumped_closure(&dir);
    let g = header_geometry(&bytes, &s);
    put_u64(&mut bytes, s.pages.start + g.slot + 8, 0);
    match refusal(&bytes) {
        ImageError::Corrupt(what) => assert!(
            what.contains("payloads"),
            "the refusal does not name the payload count: {what}"
        ),
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// The payload is a struct the verifier reads, so its pointer must be aligned,
// not merely in range. The four-byte shift keeps every extent inside the
// image — the payload is the lowest inline data, so the shifted span still
// ends below its neighbour — and only the alignment check sees it.
#[test]
fn a_misaligned_payload_is_refused() {
    let dir = crate::common::ScratchDir::new("image-header-align");
    let (mut bytes, s) = dumped_closure(&dir);
    let g = header_geometry(&bytes, &s);
    assert!(
        g.target.is_multiple_of(8),
        "the payload is not aligned at rest, so this test would shift some \
         other damage into view"
    );
    put_u64(&mut bytes, g.reloc_entry + 8, g.target as u64 + 4);
    match refusal(&bytes) {
        ImageError::Corrupt(what) => assert!(
            what.contains("misaligned"),
            "the refusal does not name the misalignment: {what}"
        ),
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

/// A lone closure whose one `MakeClosure` indexes a child, so the image
/// carries two headers and the child slot between them.
fn dumped_parent(dir: &crate::common::ScratchDir) -> (Vec<u8>, Sections) {
    dumped_value(dir, |heap, region| {
        let mut proto = TemplateProto::new(vec![7, 1, 4], Arity::Exact(0), Vec::new());
        proto.child_protos = vec![std::rc::Rc::new(TemplateProto::new(
            vec![2, 2],
            Arity::Exact(0),
            Vec::new(),
        ))];
        let template = TemplateRef::region(materialize(heap, &std::rc::Rc::new(proto), region));
        let env = heap.alloc_region_slice_in_region::<Value>(&[], region);
        heap.alloc_in_region(
            HeapObject::Closure {
                closure: Closure::new(template, env, SignalBits::EMPTY),
                traits: Value::NIL,
            },
            region,
        )
    })
}

/// Every `(entry, slot, target)` the relocation stream carries, in file
/// offsets for the entry and pages-relative offsets for the two words.
fn relocations(bytes: &[u8], s: &Sections) -> Vec<(usize, usize, usize)> {
    s.relocations
        .clone()
        .step_by(Sections::RELOC_BYTES)
        .map(|e| (e, get_u64(bytes, e) as usize, get_u64(bytes, e + 8) as usize))
        .collect()
}

/// The pages-relative offsets of every object the index gives `tag`.
fn indexed(bytes: &[u8], s: &Sections, tag: HeapTag) -> Vec<usize> {
    s.index
        .clone()
        .step_by(Sections::INDEX_BYTES)
        .filter(|&e| get_u64(bytes, e + 8) == tag as u64)
        .map(|e| get_u64(bytes, e) as usize)
        .collect()
}

// A child slot's target is read back as a header — its payload slice
// dereferenced, its blueprint word checked — so it is the one slot whose
// target must be an object the index itself calls a header. Aimed at the
// closure instance instead, every range and alignment check still passes:
// the target is an indexed object inside the image, just not this kind of
// one (docs/impl/image/sealing.md § "A child code object crosses as a
// header").
#[test]
fn a_child_slot_naming_a_non_header_is_refused() {
    let dir = crate::common::ScratchDir::new("image-child-slot");
    let (mut bytes, s) = dumped_parent(&dir);
    let relocs = relocations(&bytes, &s);
    let headers = indexed(&bytes, &s, HeapTag::ClosureTemplate);
    assert_eq!(headers.len(), 2, "the image carries a parent and a child");
    let instance = indexed(&bytes, &s, HeapTag::Closure)[0];

    // The parent is the header the closure instance's own shell names; the
    // child is the other one, and the slot naming it is the child table's.
    let shell = instance..instance + std::mem::size_of::<HeapObject>();
    let parent = relocs
        .iter()
        .find(|&&(_, slot, target)| shell.contains(&slot) && headers.contains(&target))
        .map(|&(_, _, target)| target)
        .expect("the closure names its template");
    let child = *headers
        .iter()
        .find(|&&h| h != parent)
        .expect("the parent and the child are two objects");
    let (entry, ..) = *relocs
        .iter()
        .find(|&&(_, _, target)| target == child)
        .expect("the child table names the child");

    put_u64(&mut bytes, entry + 8, instance as u64);
    match refusal(&bytes) {
        ImageError::Corrupt(what) => assert!(
            what.contains("header"),
            "the refusal does not name the header: {what}"
        ),
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A payload's own slices are extents like any object's, read through the
// admitted struct. The bytecode length is the first one behind the header:
// `CodePayload` is `repr(C)`, bytecode is its first field, and a slice's
// `len` sits eight bytes past its `ptr`.
#[test]
fn a_payload_extent_that_leaves_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-payload-extent");
    let (mut bytes, s) = dumped_closure(&dir);
    let g = header_geometry(&bytes, &s);
    let len_at = s.pages.start + g.target + 8;
    let len = u32::from_le_bytes(bytes[len_at..len_at + 4].try_into().expect("4 bytes"));
    assert_eq!(
        len, 5,
        "the bytecode length is not where repr(C) puts it, so this test \
         would damage some other field"
    );
    bytes[len_at..len_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
    match refusal(&bytes) {
        ImageError::Corrupt(what) => assert!(
            what.contains("outside the image"),
            "the refusal does not name the extent: {what}"
        ),
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}
