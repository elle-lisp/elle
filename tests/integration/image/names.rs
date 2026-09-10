// audited: 2026-09-08
// What an image's name table carries, and what a fresh instance can print
// because of it.
// docs/impl/image/format.md

use super::*;
use elle::image;

/// A spelling no static vocabulary carries, so only a name table can put it
/// in a fresh instance's memo.
const PIN: &str = "image-name-pin";

/// Render `v` the way a formatter with this instance's memo would.
fn shown(v: Value, names: &SymbolTable) -> String {
    format!("{}", v.display_with(Some(names)))
}

// § Test plan, "Names": a fresh instance prints an image's keyword by name,
// having never met the spelling. The counter-factual is the memo the test
// builds empty: with no name table in the file, the keyword still hydrates
// as an equal value and renders `#<keyword:0x…>`, which is what every
// keyword in an image did before the table existed.
#[test]
fn a_hydrated_keyword_prints_its_name() {
    let dir = crate::common::ScratchDir::new("image-name-keyword");
    let path = dir.join("keyword.image");

    let mut src = FiberHeap::new();
    let mut src_names = SymbolTable::new();
    src_names.keyword(PIN);
    image::dump(&mut src, &src_names, Value::keyword(PIN), &path).expect("dump");

    let mut dst = FiberHeap::new();
    let mut dst_names = SymbolTable::new();
    let hydrated = image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::keyword(PIN));
    assert_eq!(shown(hydrated.root, &dst_names), format!(":{PIN}"));
}

// A symbol is body data: its payload is the hash of its name, so the value
// itself crosses unchanged and the table supplies the spelling.
#[test]
fn a_hydrated_symbol_prints_its_name() {
    let dir = crate::common::ScratchDir::new("image-name-symbol");
    let path = dir.join("symbol.image");

    let mut src = FiberHeap::new();
    let mut src_names = SymbolTable::new();
    let id = src_names.intern(PIN);
    image::dump(&mut src, &src_names, Value::symbol(id), &path).expect("dump");

    let mut dst = FiberHeap::new();
    let mut dst_names = SymbolTable::new();
    let hydrated = image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::symbol(id));
    assert_eq!(shown(hydrated.root, &dst_names), PIN);
}

// Both vocabularies at once, inside a real graph rather than at the root:
// the walk has to meet a keyword and a symbol wherever they sit.
#[test]
fn the_graphs_names_reach_a_fresh_instance() {
    let dir = crate::common::ScratchDir::new("image-name-graph");
    let path = dir.join("graph.image");

    let mut src = FiberHeap::new();
    dump_graph(&mut src, &path);

    let mut dst = FiberHeap::new();
    let mut dst_names = SymbolTable::new();
    image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    assert_eq!(
        shown(Value::keyword(GRAPH_KEYWORD), &dst_names),
        format!(":{GRAPH_KEYWORD}")
    );
    assert_eq!(
        shown(Value::symbol(SymbolId::of(GRAPH_SYMBOL)), &dst_names),
        GRAPH_SYMBOL
    );
}

// § "The name table carries spellings, not hashes": a spelling the dumping
// instance never learned is absent, and the value is still a perfectly good
// value — it just has no name to print. A dumper that failed the dump here,
// or that invented a spelling, would both break the memo's contract.
#[test]
fn a_spelling_the_dumper_never_learned_still_hydrates() {
    let dir = crate::common::ScratchDir::new("image-name-unlearned");
    let path = dir.join("unlearned.image");

    let mut src = FiberHeap::new();
    let src_names = SymbolTable::new(); // met nothing at all
    image::dump(&mut src, &src_names, Value::keyword(PIN), &path).expect("dump");

    let mut dst = FiberHeap::new();
    let mut dst_names = SymbolTable::new();
    let hydrated = image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    assert_eq!(hydrated.root, Value::keyword(PIN));
    assert!(
        shown(hydrated.root, &dst_names).starts_with("#<keyword:"),
        "an unlearned spelling was invented: {}",
        shown(hydrated.root, &dst_names)
    );
}

// The table carries the body's spellings, not the dumping instance's
// vocabulary. A real instance's memo holds every identifier it ever read, so
// a dumper that emitted the memo would write thousands of names no value in
// the image names.
#[test]
fn the_table_carries_only_the_spellings_the_body_names() {
    let dir = crate::common::ScratchDir::new("image-name-scope");
    let path = dir.join("scope.image");

    let mut src = FiberHeap::new();
    let mut src_names = SymbolTable::new();
    src_names.keyword(PIN);
    src_names.intern("spelling-no-value-in-this-image-carries");
    image::dump(&mut src, &src_names, Value::keyword(PIN), &path).expect("dump");

    let bytes = std::fs::read(&path).expect("read image");
    assert!(
        find(&bytes, PIN.as_bytes()).is_some(),
        "the body's own spelling is missing from the file"
    );
    assert!(
        find(&bytes, b"spelling-no-value-in-this-image-carries").is_none(),
        "the dump copied a spelling no value in the image names"
    );
}

/// The offset of `needle` in `haystack`, if it occurs.
fn find(haystack: &[u8], needle: &[u8]) -> Option<usize> {
    haystack.windows(needle.len()).position(|w| w == needle)
}

// § Test plan, "Names" / § Dumping: two dumps of one graph are byte-identical
// whole files, and the name table is no exception. The counter-factual is the
// two memos: they hold the same spellings in opposite insertion orders, so a
// dumper that emitted its name table in memo-iteration order writes two
// different files from one graph.
#[test]
fn the_name_table_does_not_depend_on_memo_order() {
    let dir = crate::common::ScratchDir::new("image-name-order");
    let (a, b) = (dir.join("a.image"), dir.join("b.image"));

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let root = build_graph(&mut src, region);

    let mut forward = SymbolTable::new();
    forward.keyword(GRAPH_KEYWORD);
    forward.intern(GRAPH_SYMBOL);
    let mut backward = SymbolTable::new();
    backward.intern(GRAPH_SYMBOL);
    backward.keyword(GRAPH_KEYWORD);

    image::dump(&mut src, &forward, root, &a).expect("dump a");
    image::dump(&mut src, &backward, root, &b).expect("dump b");
    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "the name table depends on the order its memo learned the spellings"
    );
}

// Replay is a learning site, and a learning site meets names it already
// holds: the second image here carries a spelling the first one taught this
// instance. The trap: the collision guard panics on a hash the memo maps to
// a different spelling, so a replay that recorded without comparing first
// would abort every hydration of a second image into one instance.
#[test]
fn a_second_image_replays_a_spelling_the_instance_already_holds() {
    let dir = crate::common::ScratchDir::new("image-name-twice");
    let path = dir.join("twice.image");

    let mut src = FiberHeap::new();
    let mut src_names = SymbolTable::new();
    src_names.keyword(PIN);
    image::dump(&mut src, &src_names, Value::keyword(PIN), &path).expect("dump");

    let mut dst = FiberHeap::new();
    let mut dst_names = SymbolTable::new();
    let first = image::hydrate_path(&mut dst, &mut dst_names, &path).expect("first hydrate");
    let second = image::hydrate_path(&mut dst, &mut dst_names, &path).expect("second hydrate");
    assert_eq!(shown(first.root, &dst_names), format!(":{PIN}"));
    assert_eq!(shown(second.root, &dst_names), format!(":{PIN}"));
}
