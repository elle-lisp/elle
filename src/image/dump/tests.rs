// audited: 2026-09-11
//! What a dump records that no caller outside this crate can build: a syntax
//! tree's scope sets, and the port behind a parameter's default.
//!
//! docs/impl/image/sealing.md
//! docs/impl/image/format.md

use std::path::PathBuf;

use crate::hir::region::RuntimeRegion;
use crate::port::{Direction, Encoding, Port, PortId, PortKind};
use crate::symbol::SymbolTable;
use crate::syntax::ScopeId;
use crate::syntax::{Span, Syntax, SyntaxArena};
use crate::value::fiberheap::FiberHeap;
use crate::value::heap::{deref, HeapObject};
use crate::value::Value;

use super::super::{ImageError, Sections};

/// A scratch file this test owns, removed when it goes out of scope.
struct Scratch(PathBuf);

impl Scratch {
    fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("elle-image-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).expect("scratch dir");
        Scratch(dir.join("tree.image"))
    }
}

impl Drop for Scratch {
    fn drop(&mut self) {
        if let Some(dir) = self.0.parent() {
            let _ = std::fs::remove_dir_all(dir);
        }
    }
}

/// Dump `build`'s value and hydrate it into a second heap, answering that
/// heap and what hydration reported. The heap comes back because the tree
/// lives in its mapped pages.
fn round_trip(
    build: impl FnOnce(&mut FiberHeap, &SyntaxArena) -> Value,
    tag: &str,
) -> (FiberHeap, super::super::Hydrated) {
    let scratch = Scratch::new(tag);
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arena = SyntaxArena::new(&mut src, region);
    let root = build(&mut src, &arena);
    super::dump(&mut src, &SymbolTable::new(), root, &scratch.0).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated =
        super::super::hydrate_path(&mut dst, &mut SymbolTable::new(), &scratch.0).expect("hydrate");
    (dst, hydrated)
}

/// Wrap `tree` in a syntax value, as `value::build::syntax` does.
fn syntax_value(heap: &mut FiberHeap, arena: &SyntaxArena, tree: Syntax) -> Value {
    let owned = tree.copy_into(arena);
    heap.alloc_in_region(
        HeapObject::Syntax {
            syntax: owned,
            traits: Value::NIL,
        },
        arena.region(),
    )
}

fn tree_of(v: Value) -> &'static Syntax {
    let HeapObject::Syntax { syntax, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not syntax");
    };
    syntax
}

// A node's scope set is what hygiene compares, so it crosses like any other
// inline slice. Nothing outside this crate can build a scoped node, which is
// why this pin lives here rather than beside the other round-trip tests.
#[test]
fn a_nodes_scope_set_survives_hydration() {
    let scopes = [ScopeId(3), ScopeId::intro(5), ScopeId(7)];
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            let node = Syntax::symbol_scoped(arena, "scoped", Span::synthetic(), &scopes);
            syntax_value(heap, arena, node)
        },
        "scopes",
    );
    assert_eq!(tree_of(hydrated.root).scopes(), scopes);
}

// § "The scope watermark bounds what a fresh expander may mint": an expander
// starts its counter at one, so a hydrated body's scopes and a fresh
// expander's would collide. The watermark clears every counter value in the
// body — the intro scope included, since intro ids and ordinary ones come off
// the same counter and differ only in a reserved bit.
//
// The counter-factual is `intro(9)`: read as a raw id it is a number near
// `u32::MAX`, so a watermark that did not mask the bit would answer with a
// counter no expander could ever reach.
#[test]
fn the_scope_watermark_clears_every_counter_in_the_body() {
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            let scopes = [ScopeId(4), ScopeId::intro(9)];
            let node = Syntax::symbol_scoped(arena, "scoped", Span::synthetic(), &scopes);
            syntax_value(heap, arena, node)
        },
        "watermark",
    );
    assert_eq!(hydrated.scope_watermark, 10);
}

// A body with no syntax carries no scopes, so a loader may mint from its own
// start. Zero says exactly that, and a dump that reported one anyway would
// push every later expander's counter up for nothing.
#[test]
fn a_body_without_syntax_has_no_watermark() {
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            heap.alloc_in_region(
                HeapObject::Pair(crate::value::heap::Pair::new(
                    Value::int(1),
                    Value::EMPTY_LIST,
                )),
                arena.region(),
            )
        },
        "no-watermark",
    );
    assert_eq!(hydrated.scope_watermark, 0);
}

// ── A parameter and the resource behind its default ──────────────────

/// A parameter carrying `default`, allocated into `region`.
fn parameter_in(heap: &mut FiberHeap, region: RuntimeRegion, id: u32, default: Value) -> Value {
    heap.alloc_in_region(
        HeapObject::Parameter {
            id,
            default,
            traits: Value::NIL,
        },
        region,
    )
}

/// The id and the default of a parameter value.
fn parameter_of(v: Value) -> (u32, Value) {
    let HeapObject::Parameter { id, default, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not a parameter");
    };
    (*id, *default)
}

/// What a `"port"` external says about itself: which stream it is, and which
/// port it is. Both by value, because an external's payload is borrowed from
/// the `Value` the caller holds.
fn port_of(v: Value) -> (PortKind, PortId) {
    assert_eq!(v.external_type_name(), Some("port"), "not a port external");
    let port = v
        .as_external::<Port>()
        .expect("a port external holds a Port");
    (port.kind(), port.id())
}

/// Dump a parameter whose default is a fresh stdout port, as the image's only
/// reconstruction. Answers the scratch file and the dumped port's identity, so
/// a caller can ask whether the hydrated port is that one or another.
fn dump_stdio_parameter(tag: &str, id: u32) -> (Scratch, PortId) {
    let scratch = Scratch::new(tag);
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let default = crate::value::build::external(&mut src, "port", Port::stdout(), region);
    let root = parameter_in(&mut src, region, id, default);
    super::dump(&mut src, &SymbolTable::new(), root, &scratch.0).expect("dump");
    (scratch, port_of(default).1)
}

/// Hydrate `scratch` into a fresh heap, answering both.
fn hydrate(scratch: &Scratch) -> (FiberHeap, super::super::Hydrated) {
    let mut dst = FiberHeap::new();
    let hydrated =
        super::super::hydrate_path(&mut dst, &mut SymbolTable::new(), &scratch.0).expect("hydrate");
    (dst, hydrated)
}

// A `Parameter` is sealed POD, so a default that is ordinary data crosses in
// the body like the contents of any other object, and the id crosses beside
// it.
#[test]
fn a_parameters_data_default_round_trips() {
    let scratch = Scratch::new("param-data");
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = parameter_in(&mut src, region, 7, Value::int(42));
    super::dump(&mut src, &SymbolTable::new(), root, &scratch.0).expect("dump");

    let (_dst, hydrated) = hydrate(&scratch);
    assert_eq!(parameter_of(hydrated.root), (7, Value::int(42)));
}

// A stdio port is process-owned, so the hydrating instance opens its own.
//
// The counter-factual is the port identity. A stdout port answers `port/write`
// the same way whoever made it, so a hydrated default that was somehow carried
// across would pass every behavioral check — and hold a `Port` built by a
// process that has exited. Comparing identities is what tells a rebuilt port
// from a carried one.
#[test]
fn a_stdio_default_is_rebuilt_by_the_hydrating_instance() {
    let (scratch, dumped) = dump_stdio_parameter("param-stdio", 1);
    let (dst, hydrated) = hydrate(&scratch);

    let (_, default) = parameter_of(hydrated.root);
    let (kind, id) = port_of(default);
    assert_eq!(kind, PortKind::Stdout, "the stream changed");
    assert_ne!(id, dumped, "the hydrated default is the dumped port");

    let ptr = default.as_heap_ptr().expect("the default is a heap value");
    assert_ne!(
        dst.region_of_ptr(ptr),
        hydrated.region.get(),
        "the reconstructed port landed in the image's own pages"
    );
}

// A reconstruction writes a pointer into another region, which is a
// cross-region reference like any other: counted once at hydration and
// released once by the free cascade. The trap is the free-time equivalence
// oracle — a debug build scans the dying region and aborts when the recorded
// edge table disagrees with what it finds.
#[test]
fn a_reconstructed_default_is_counted_once() {
    let (scratch, _) = dump_stdio_parameter("param-edge", 1);
    let mut dst = FiberHeap::new();
    let before = dst.active_region_count();
    let hydrated =
        super::super::hydrate_path(&mut dst, &mut SymbolTable::new(), &scratch.0).expect("hydrate");

    let (_, default) = parameter_of(hydrated.root);
    let ptr = default.as_heap_ptr().expect("the default is a heap value");
    let companion =
        RuntimeRegion::new(dst.region_of_ptr(ptr)).expect("the port lives in a real region");
    assert_eq!(
        dst.region_rc(companion),
        1,
        "the reconstructed port's region is held by something besides the edge"
    );

    dst.decref_region_if_present(hydrated.region);
    assert_eq!(
        dst.region_rc(companion),
        0,
        "freeing the hydrated region did not release the reconstructed port"
    );
    assert_eq!(
        dst.active_region_count(),
        before,
        "a hydration left a region behind"
    );
}

// Only the three standard streams reconstruct. A file port owns a descriptor
// this process opened, and the name it was opened under promises nothing about
// what another process would find there — so the dump fails by name rather
// than writing an entry no load can answer.
#[test]
fn a_file_port_default_refuses_the_dump() {
    let scratch = Scratch::new("param-file-port");
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let port = Port::new_unopened(
        PortKind::File,
        Direction::Read,
        Encoding::Text,
        "image.lisp".to_string(),
    );
    let default = crate::value::build::external(&mut src, "port", port, region);
    let root = parameter_in(&mut src, region, 2, default);

    match super::dump(&mut src, &SymbolTable::new(), root, &scratch.0) {
        Err(ImageError::Unsupported(what)) => assert!(
            what.contains("File"),
            "the refusal does not name the kind of port: {what}"
        ),
        other => panic!("expected a refused dump, got {other:?}"),
    }
    assert!(!scratch.0.exists(), "a refused dump left a partial file");
}

// The dumper zeroes a reconstructed slot's whole `Value`, both words, so the
// artifact records nothing about the resource the dumping process held. Two
// builds whose ports land at different addresses then write one file.
#[test]
fn the_artifact_records_no_port_address() {
    let (scratch, _) = dump_stdio_parameter("param-zero", 1);
    let bytes = std::fs::read(&scratch.0).expect("read image");
    let s = super::super::sections(&bytes).expect("a freshly dumped image parses");

    assert_eq!(
        s.reconstruction.len(),
        Sections::RECON_BYTES,
        "the image should name exactly one reconstruction slot"
    );
    let slot = u64::from_le_bytes(
        bytes[s.reconstruction.start..s.reconstruction.start + 8]
            .try_into()
            .expect("8 bytes"),
    ) as usize;
    let at = s.pages.start + slot;
    let width = std::mem::size_of::<Value>();
    assert_eq!(
        &bytes[at..at + width],
        vec![0u8; width],
        "the dumped slot still carries the dumping instance's port"
    );
}

// A parameter resolves by id, so a fresh instance minting from its own counter
// would hand out an id the body already uses, and `parameterize` over either
// one would rebind both. The watermark is what stops that, and hydration
// raises the process counter past it.
//
// The id is far above anything a test run mints, so `peek` can only exceed it
// because this image moved the counter.
#[test]
fn the_parameter_watermark_clears_every_id_in_the_body() {
    const ID: u32 = 4_000_000;
    let (scratch, _) = dump_stdio_parameter("param-watermark", ID);
    let (_dst, hydrated) = hydrate(&scratch);

    assert_eq!(hydrated.param_watermark, ID + 1);
    assert!(
        crate::value::parameter::peek() > ID,
        "hydration left the process minting ids the image already uses"
    );
}

// A body with no parameter moves no counter. Zero says that, and a dump that
// reported one anyway would push every later mint up for nothing.
#[test]
fn a_body_without_parameters_has_no_watermark() {
    let (_dst, hydrated) = round_trip(
        |heap, arena| {
            heap.alloc_in_region(
                HeapObject::Pair(crate::value::heap::Pair::new(
                    Value::int(1),
                    Value::EMPTY_LIST,
                )),
                arena.region(),
            )
        },
        "no-param-watermark",
    );
    assert_eq!(hydrated.param_watermark, 0);
}
