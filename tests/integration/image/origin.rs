// audited: 2026-09-14
// The defining span crosses on the payload, so `meta/origin` answers after a
// hydration.
// docs/impl/image/sealing.md
// docs/impl/image/plan.md

use std::rc::Rc;

use super::*;
use elle::pipeline::eval_all;
use elle::syntax::Span;
use elle::value::Arity;

/// A file name this test process has no other reason to intern, so the image's
/// file table is the only place the hydrated span's spelling can come from.
const ORIGIN_FILE: &str = "aaa-image-origin.lisp";

/// The span the lambdas below record as their origin.
fn origin_span() -> Span {
    Span::new(120, 140, 12, 5).with_file(ORIGIN_FILE)
}

/// A one-instruction lambda whose blueprint records `origin`.
fn proto_with_origin(origin: Option<Span>) -> Rc<TemplateProto> {
    let mut proto = TemplateProto::new(vec![1], Arity::Exact(0), Vec::new());
    proto.origin = origin;
    Rc::new(proto)
}

/// Dump a closure over `proto` to `path` and hydrate it into a fresh heap,
/// answering the hydrated root. The heap stays alive for the caller, because
/// reading the root dereferences its pages.
fn round_trip(proto: &Rc<TemplateProto>, path: &std::path::Path) -> (FiberHeap, Value) {
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = closure_in(&mut src, region, proto, &[], SignalBits::EMPTY);
    image::dump(&mut src, &SymbolTable::new(), root, path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), path).expect("hydrate");
    (dst, hydrated.root)
}

// § Test plan, "Closures": a hydrated closure reports the line, the column and
// the file it was written at. Every field of the span crosses, not only the
// three `meta/origin` prints, because the span is carried whole.
#[test]
fn a_hydrated_header_answers_the_origin_its_source_did() {
    let dir = crate::common::ScratchDir::new("image-origin-fields");
    let path = dir.join("origin.image");

    let proto = proto_with_origin(Some(origin_span()));
    let (_dst, root) = round_trip(&proto, &path);
    assert_eq!(
        closure_of(root).template.origin(),
        Some(origin_span()),
        "the hydrated header lost the span its source header answered"
    );
}

// § "A span names its file by name, not by id": the file table decides which
// file a hydrated origin names. The counter-factual is the patch — renaming
// the spelling in the table renames the file the origin reports. Without the
// file stream the hydrated span keeps its dumped id, which in this one process
// still resolves, to the original name.
#[test]
fn a_hydrated_origin_names_the_file_the_table_spells() {
    let dir = crate::common::ScratchDir::new("image-origin-file");
    let path = dir.join("origin.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let proto = proto_with_origin(Some(origin_span()));
    let root = closure_in(&mut src, region, &proto, &[], SignalBits::EMPTY);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut bytes = std::fs::read(&path).expect("read image");
    let sections = image::sections(&bytes).expect("a freshly dumped image parses");
    let renamed = "bbb-image-origin.lisp";
    assert_eq!(
        renamed.len(),
        ORIGIN_FILE.len(),
        "the patch must not move any byte"
    );
    let found = bytes[sections.files.clone()]
        .windows(ORIGIN_FILE.len())
        .position(|w| w == ORIGIN_FILE.as_bytes())
        .expect("the file table spells the origin's file");
    let start = sections.files.start + found;
    bytes[start..start + renamed.len()].copy_from_slice(renamed.as_bytes());

    let source = image::ImageSource::from_bytes(&bytes).expect("anonymous file");
    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &mut SymbolTable::new(), &source).expect("hydrate");
    assert_eq!(
        closure_of(hydrated.root)
            .template
            .origin()
            .expect("the hydrated header answers an origin")
            .file(),
        Some(renamed),
    );
}

// A lambda the reader never saw has no origin, and absence is an answer of its
// own. The trap: a file stream that listed every payload, absent spans
// included, would write the table's first entry over the absent id and give a
// generated lambda a source file it never had.
#[test]
fn a_lambda_with_no_origin_still_answers_none() {
    let dir = crate::common::ScratchDir::new("image-origin-absent");
    let path = dir.join("none.image");

    // A second closure carries a real origin, so the file table is not empty
    // and the first entry is there to be written wrongly.
    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let bare = closure_in(
        &mut src,
        region,
        &proto_with_origin(None),
        &[],
        SignalBits::EMPTY,
    );
    let named = closure_in(
        &mut src,
        region,
        &proto_with_origin(Some(origin_span())),
        &[],
        SignalBits::EMPTY,
    );
    let root = alloc_pair(&mut src, region, bare, named);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let HeapObject::Pair(pair) = (unsafe { deref(hydrated.root) }) else {
        panic!("the hydrated root is not a pair");
    };
    assert_eq!(
        closure_of(pair.first).template.origin(),
        None,
        "a lambda with no origin hydrated carrying one"
    );
    assert_eq!(
        closure_of(pair.rest)
            .template
            .origin()
            .and_then(|s| s.file()),
        Some(ORIGIN_FILE),
    );
}

// § Test plan, "Closures": `meta/origin` still answers. The end-to-end pin —
// a lambda compiled in one runtime, hydrated into a fresh one, and asked
// through the ordinary dispatch path for the file and line it was written at.
#[test]
fn meta_origin_answers_for_a_hydrated_closure() {
    let dir = crate::common::ScratchDir::new("image-origin-meta");
    let path = dir.join("compiled.image");
    const FILE: &str = "aaa-image-origin-meta.lisp";

    let mut rt = Runtime::new();
    let f = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all("(fn [x] (if (%eq x 2) 42 7))", symbols, vm, cctx, FILE).expect("eval")
    };
    assert!(
        f.as_closure()
            .expect("the eval produced a closure")
            .template
            .origin()
            .is_some(),
        "a compiled lambda carries an origin before the dump"
    );
    {
        let (heap, symbols) = rt.heap_and_symbols();
        image::dump(heap, symbols, f, &path).expect("dump");
    }

    let mut rt2 = Runtime::new();
    bind_hydrated(&mut rt2, &path);
    let (vm, symbols, cctx) = rt2.parts();
    let file = eval_all(
        &format!("(= (get (meta/origin hydrated-f) :file) \"{FILE}\")"),
        symbols,
        vm,
        cctx,
        "<image-origin>",
    )
    .expect("meta/origin");
    assert_eq!(file, Value::TRUE, "meta/origin named the wrong file");
    let line = eval_all(
        "(get (meta/origin hydrated-f) :line)",
        symbols,
        vm,
        cctx,
        "<image-origin>",
    )
    .expect("meta/origin");
    assert_eq!(line.as_int(), Some(1), "meta/origin named the wrong line");
}
