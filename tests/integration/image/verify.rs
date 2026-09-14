// audited: 2026-09-13
// What the verifier refuses: a table entry that would send a write, a read,
// or a rebuild outside the image.
// docs/impl/image.md

use super::*;
use elle::image::{self, ImageError, Sections};
use elle::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};
use elle::value::closure::materialize;
use elle::value::fiber::SignalBits;
use elle::value::heap::Closure;
use elle::value::{Arity, HeapTag, TemplateProto, TemplateRef};

/// Dump the standard graph and answer its bytes, with the section ranges the
/// tests below damage.
fn dumped_bytes(dir: &crate::common::ScratchDir) -> (Vec<u8>, Sections) {
    let path = dir.join("graph.image");
    let mut src = FiberHeap::new();
    dump_graph(&mut src, &path);
    let bytes = std::fs::read(&path).expect("read image");
    let sections = image::sections(&bytes).expect("a freshly dumped image parses");
    (bytes, sections)
}

/// Dump one lone value and answer its bytes and section ranges. The graph is
/// the caller's, so a test can put a single object in the image and know
/// exactly where its inline data sits.
fn dumped_value(
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

/// A heap with its default trait tables built, as VM init leaves one — the
/// instance an image is dumped from and hydrated into.
fn traited_heap() -> FiberHeap {
    let mut heap = FiberHeap::new();
    elle::primitives::traitregistry::init_default_traits(&mut heap);
    heap
}

/// Hydrate `bytes` in a fresh heap and answer the refusal, asserting the
/// failed load left neither a region nor page bytes behind.
fn refusal(bytes: &[u8]) -> ImageError {
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

fn put_u64(bytes: &mut [u8], at: usize, v: u64) {
    bytes[at..at + 8].copy_from_slice(&v.to_le_bytes());
}

fn get_u64(bytes: &[u8], at: usize) -> u64 {
    u64::from_le_bytes(bytes[at..at + 8].try_into().expect("8 bytes"))
}

// A relocation slot names where hydration writes an address. Out of range, it
// writes outside the image — the one thing a corrupt table must never buy.
#[test]
fn a_relocation_slot_outside_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-reloc-range");
    let (mut bytes, s) = dumped_bytes(&dir);
    assert!(!s.relocations.is_empty(), "the graph has relocations to damage");
    let pages_len = s.pages.len() as u64;
    put_u64(&mut bytes, s.relocations.start, pages_len);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// Every relocation slot is a `Value` payload or a `RegionSlice` ptr, and both
// are 8-byte aligned by construction. The trap: hydration writes the slot
// with an unaligned store, which succeeds on x86-64 and silently overwrites
// half of each of two neighbouring fields — corruption a range check cannot
// see, because the offset is inside the image.
#[test]
fn a_misaligned_relocation_slot_is_refused() {
    let dir = crate::common::ScratchDir::new("image-reloc-align");
    let (mut bytes, s) = dumped_bytes(&dir);
    let slot = get_u64(&bytes, s.relocations.start);
    put_u64(&mut bytes, s.relocations.start, slot + 1);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A `RegionSlice` is a `(ptr, len)` pair, and the length is not relocated —
// it is copied straight out of the file. An overlong length is a read past
// the image at the first use of that string, array, or byte sequence, long
// after hydration returned success.
#[test]
fn a_slice_extent_that_leaves_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-slice-extent");
    let (mut bytes, s) = dumped_bytes(&dir);

    // The graph's one string is "hello image". Find its object through the
    // index, then its slice header: a probed variant's payload sits at offset
    // 8 of the shell, and a `RegionSlice`'s `len` is 8 bytes into that
    // (docs/impl/image/measurements.md item 6).
    let mut patched = false;
    for entry in s.index.clone().step_by(Sections::INDEX_BYTES) {
        if get_u64(&bytes, entry + 8) != HeapTag::LString as u64 {
            continue;
        }
        let len_at = s.pages.start + get_u64(&bytes, entry) as usize + 16;
        let len = u32::from_le_bytes(bytes[len_at..len_at + 4].try_into().expect("4 bytes"));
        assert_eq!(
            len,
            "hello image".len() as u32,
            "the string's length is not where the layout probes say it is, \
             so this test would damage some other field"
        );
        bytes[len_at..len_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());
        patched = true;
        break;
    }
    assert!(patched, "no LString in the index to damage");

    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A struct's entries are a slice like any other, but its element is a
// `(TableKey, Value)` — 40 bytes, not the 16 a `Value` slice uses. The
// counter-factual is the length this test writes: one element past the end,
// which overruns the page at the true stride and stays comfortably inside it
// at a `Value`'s. A verifier that measured the extent in `Value`s would pass
// the image and let the first probe of that struct read past its page.
//
// The struct is the image's only object, so its entries are the only inline
// data and they end exactly at the page's end — which is what makes one extra
// element an overrun rather than a read of the neighbour's bytes.
#[test]
fn a_struct_entry_slice_that_leaves_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-entry-extent");
    let (mut bytes, s) = dumped_value(&dir, |heap, region| {
        alloc_struct(
            heap,
            region,
            &[
                (TableKey::Int(1), Value::int(2)),
                (TableKey::Int(3), Value::int(4)),
            ],
        )
    });

    let mut patched = false;
    for entry in s.index.clone().step_by(Sections::INDEX_BYTES) {
        if get_u64(&bytes, entry + 8) != HeapTag::LStruct as u64 {
            continue;
        }
        let len_at = s.pages.start + get_u64(&bytes, entry) as usize + 16;
        let len = u32::from_le_bytes(bytes[len_at..len_at + 4].try_into().expect("4 bytes"));
        assert_eq!(
            len, 2,
            "the entry count is not where the layout probes say it is, \
             so this test would damage some other field"
        );
        bytes[len_at..len_at + 4].copy_from_slice(&3u32.to_le_bytes());
        patched = true;
        break;
    }
    assert!(patched, "no LStruct in the index to damage");

    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A syntax object's root node rides in its shell, so its child slice is an
// extent the walk reads there (docs/impl/image.md § Verifier). The trap: a
// verifier that checked only the one extent a string, array, or struct has
// would skip a syntax object entirely, and the first walk of that tree would
// read past the image.
//
// The tree is the image's only object and carries no scopes, so the one
// relocation in the file is the root node's child pointer — which is what
// lets this test find the length beside it without a layout probe.
#[test]
fn a_syntax_child_extent_that_leaves_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-syntax-extent");
    let (mut bytes, s) = dumped_value(&dir, |heap, region| {
        let arena = SyntaxArena::new(heap, region);
        let kids = [
            Syntax::new(SyntaxKind::Int(1), Span::synthetic()),
            Syntax::new(SyntaxKind::Int(2), Span::synthetic()),
            Syntax::new(SyntaxKind::Int(3), Span::synthetic()),
        ];
        let tree = Syntax::list(&arena, &kids, Span::synthetic());
        let owned = tree.copy_into(&arena);
        heap.alloc_in_region(
            HeapObject::Syntax {
                syntax: owned,
                traits: Value::NIL,
            },
            region,
        )
    });
    assert_eq!(
        s.relocations.len(),
        Sections::RELOC_BYTES,
        "the lone tree has one relocation, the root node's child pointer"
    );

    // A `RegionSlice`'s len sits 8 bytes past its ptr, and the ptr is the
    // relocated slot.
    let len_at = s.pages.start + get_u64(&bytes, s.relocations.start) as usize + 8;
    let len = u32::from_le_bytes(bytes[len_at..len_at + 4].try_into().expect("4 bytes"));
    assert_eq!(len, 3, "the child count is not beside the relocated pointer");
    bytes[len_at..len_at + 4].copy_from_slice(&u32::MAX.to_le_bytes());

    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A name entry is a length and its bytes. A length past the section's end
// reads spellings out of whatever follows the image, and the names section is
// the last one — so the read runs off the buffer entirely.
#[test]
fn a_name_longer_than_its_section_is_refused() {
    let dir = crate::common::ScratchDir::new("image-name-length");
    let (mut bytes, s) = dumped_bytes(&dir);
    assert!(!s.names.is_empty(), "the graph has spellings to damage");
    put_u64(&mut bytes, s.names.start, s.names.len() as u64);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A spelling is a Rust `&str` the moment the memo records it. Bytes that are
// not UTF-8 have no such reading, so the table is refused rather than
// lossily repaired into a name that denotes some other symbol.
#[test]
fn a_name_that_is_not_utf8_is_refused() {
    let dir = crate::common::ScratchDir::new("image-name-utf8");
    let (mut bytes, s) = dumped_bytes(&dir);
    let len = get_u64(&bytes, s.names.start) as usize;
    assert!(len > 0, "the first name entry is empty");
    bytes[s.names.start + 8] = 0xFF;
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

/// An array carrying its instance's default traitset, holding one native-fn:
/// the smallest image with a reconstruction entry and a primitive slot.
fn dumped_traited_array(dir: &crate::common::ScratchDir) -> (Vec<u8>, Sections) {
    dumped_value(dir, |heap, region| {
        let prim = Value::native_fn(
            elle::primitives::prim_table_snapshot()
                .into_iter()
                .find(|d| d.name == "insert")
                .expect("insert is a canonical primitive"),
        );
        let slice = heap.alloc_region_slice_in_region(&[prim], region);
        let traits = heap.default_traits_for(HeapTag::LArray);
        heap.alloc_in_region(
            HeapObject::LArray {
                elements: slice,
                traits,
            },
            region,
        )
    })
}

// A primitive slot names where hydration writes a resolved id. Out of range,
// it writes outside the image, exactly as a pointer relocation would.
#[test]
fn a_primitive_slot_outside_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-prim-range");
    let (mut bytes, s) = dumped_traited_array(&dir);
    assert!(!s.prim_slots.is_empty(), "the image names a primitive slot");
    put_u64(&mut bytes, s.prim_slots.start, s.pages.len() as u64);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// The second half of a primitive entry is an index into the primitive table.
// An index past the table names no spelling, so there is no primitive to
// resolve and nothing to write.
#[test]
fn a_primitive_slot_naming_no_spelling_is_refused() {
    let dir = crate::common::ScratchDir::new("image-prim-index");
    let (mut bytes, s) = dumped_traited_array(&dir);
    put_u64(&mut bytes, s.prim_slots.start + 8, 99);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// A reconstruction slot takes a whole `Value` — sixteen bytes, not eight — so
// its bound is the wider one. The counter-factual is a slot eight bytes below
// the end of the pages: a verifier that bounded it like a pointer relocation
// would admit it, and hydration would write the payload word past the image.
#[test]
fn a_reconstruction_slot_outside_the_image_is_refused() {
    let dir = crate::common::ScratchDir::new("image-recon-range");
    let (mut bytes, s) = dumped_traited_array(&dir);
    assert!(
        !s.reconstruction.is_empty(),
        "the traited array has a reconstruction entry"
    );
    put_u64(&mut bytes, s.reconstruction.start, s.pages.len() as u64 - 8);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// The constructor tag says which value to build. A tag this binary has no
// constructor for is format drift, refused at the table rather than left to
// write whatever a mis-read tag resolves to.
#[test]
fn an_unknown_constructor_tag_is_refused() {
    let dir = crate::common::ScratchDir::new("image-recon-ctor");
    let (mut bytes, s) = dumped_traited_array(&dir);
    put_u64(&mut bytes, s.reconstruction.start + 8, u64::MAX);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// § Verifier: an immediate root and a pages section disagree. The trap is
// what the root's payload word means — a page offset for a heap root, the
// value's own payload for an immediate one, and a primitive-table index for a
// native-fn. A file claiming both leaves that reading to whichever branch runs
// last, which is how a payload becomes a pointer nobody minted.
#[test]
fn an_immediate_root_beside_pages_is_refused() {
    let dir = crate::common::ScratchDir::new("image-root-kind");
    let (mut bytes, s) = dumped_bytes(&dir);
    assert!(!s.pages.is_empty(), "the graph has pages to disagree with");

    // The header's root fields: the tag word at 48, the heap flag at 64. The
    // flag is read back first, so a header whose layout moved fails here
    // rather than damaging some other field.
    assert_eq!(get_u64(&bytes, 64), 1, "the dumped graph has a heap root");
    put_u64(&mut bytes, 48, 0); // an int's tag
    put_u64(&mut bytes, 64, 0);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

// The page table's cursors are what the rebuilt region allocates from. A
// cursor below the objects the index places on that page hands the next
// allocation an address an image object already occupies, so the region
// starts overwriting its own contents.
#[test]
fn a_page_cursor_that_disagrees_with_the_index_is_refused() {
    let dir = crate::common::ScratchDir::new("image-cursor");
    let (mut bytes, s) = dumped_bytes(&dir);
    assert!(!s.page_table.is_empty(), "the image has a page to damage");

    // Entry 0 is (size, obj_cursor, data_cursor); 16 is the header's end, so
    // this claims the page holds no objects at all.
    put_u64(&mut bytes, s.page_table.start + 8, 16);
    match refusal(&bytes) {
        ImageError::Corrupt(_) => {}
        other => panic!("expected a corrupt-image refusal, got {other:?}"),
    }
}

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
