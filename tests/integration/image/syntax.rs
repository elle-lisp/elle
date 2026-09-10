// audited: 2026-09-08
// What survives a syntax tree's round trip: its shape, its spans, and the
// file each span names.
// docs/impl/image/format.md

use super::*;
use elle::image;
use elle::syntax::{Span, Syntax, SyntaxArena, SyntaxKind};
use elle::value::heap::deref;

/// A file name this test process has no other reason to intern.
const FILE: &str = "aaa-image-syntax.lisp";

/// Build `tree` into `region` and wrap it in a syntax value, the way
/// `value::build::syntax` does for a first-class syntax object.
fn alloc_syntax(heap: &mut FiberHeap, region: RuntimeRegion, tree: Syntax) -> Value {
    let arena = SyntaxArena::new(heap, region);
    let owned = tree.copy_into(&arena);
    heap.alloc_in_region(
        HeapObject::Syntax {
            syntax: owned,
            traits: Value::NIL,
        },
        region,
    )
}

/// The tree inside a syntax value.
fn tree_of(v: Value) -> &'static Syntax {
    let HeapObject::Syntax { syntax, .. } = (unsafe { deref(v) }) else {
        panic!("the value is not syntax");
    };
    syntax
}

/// A span at `line`, naming [`FILE`].
fn at(line: u32) -> Span {
    Span::new(line as usize * 10, line as usize * 10 + 4, line, 1).with_file(FILE)
}

/// One node of every shape the reader produces, nested: atoms, a sequence, a
/// wrapping kind, and the region strings three kinds carry.
fn build_tree(arena: &SyntaxArena) -> Syntax {
    let atoms = [
        Syntax::symbol(arena, "an-image-symbol", at(1)),
        Syntax::keyword(arena, "an-image-keyword", at(2)),
        Syntax::string(arena, "an image string", at(3)),
        Syntax::string_mut(arena, "a mutable one", at(4)),
        Syntax::new(SyntaxKind::Int(-7), at(5)),
        Syntax::new(SyntaxKind::Float(2.5), at(6)),
        Syntax::new(SyntaxKind::Bool(true), at(7)),
        Syntax::new(SyntaxKind::Nil, at(8)),
    ];
    let inner = Syntax::array(arena, &atoms, at(9));
    let quoted = Syntax::quote(arena, inner, at(10));
    Syntax::list(arena, &[quoted, Syntax::symbol(arena, "tail", at(11))], at(12))
}

/// Structural equality over two trees: same kinds, same payloads, same spans,
/// same flags, same children in the same order.
fn same_tree(a: &Syntax, b: &Syntax) -> bool {
    if a.span != b.span || a.scope_exempt != b.scope_exempt || !same_kind(&a.kind, &b.kind) {
        return false;
    }
    if let (SyntaxKind::SyntaxLiteral(x), SyntaxKind::SyntaxLiteral(y)) = (&a.kind, &b.kind) {
        // A syntax literal reports no children, so its one child is compared
        // here or nowhere.
        return same_tree(x, y);
    }
    let (ka, kb) = (a.kind.children(), b.kind.children());
    ka.len() == kb.len() && ka.iter().zip(kb).all(|(x, y)| same_tree(x, y))
}

/// The kinds agree in variant and in whatever payload is not a child.
fn same_kind(a: &SyntaxKind, b: &SyntaxKind) -> bool {
    use SyntaxKind::*;
    match (a, b) {
        (Nil, Nil) => true,
        (Bool(x), Bool(y)) => x == y,
        (Int(x), Int(y)) => x == y,
        (Float(x), Float(y)) => x.to_bits() == y.to_bits(),
        (Symbol(x), Symbol(y))
        | (Keyword(x), Keyword(y))
        | (String(x), String(y))
        | (StringMut(x), StringMut(y)) => x.as_str() == y.as_str(),
        // The compound kinds carry only their children, which the caller
        // compares; matching variants is all that is left to check here.
        (List(_), List(_))
        | (Array(_), Array(_))
        | (ArrayMut(_), ArrayMut(_))
        | (Struct(_), Struct(_))
        | (StructMut(_), StructMut(_))
        | (Set(_), Set(_))
        | (SetMut(_), SetMut(_))
        | (Bytes(_), Bytes(_))
        | (BytesMut(_), BytesMut(_))
        | (Quote(_), Quote(_))
        | (Quasiquote(_), Quasiquote(_))
        | (Unquote(_), Unquote(_))
        | (UnquoteSplicing(_), UnquoteSplicing(_))
        | (Splice(_), Splice(_))
        | (SyntaxLiteral(_), SyntaxLiteral(_)) => true,
        _ => false,
    }
}

// § Sealing: syntax is body data, so a tree crosses as page bytes. The trap a
// structural comparison catches: a `Value` holding syntax compares by
// identity, so the round-trip pin every other value gets — assert the
// hydrated value equals its source — says nothing here and would pass over a
// tree that lost half its children.
#[test]
fn a_syntax_tree_round_trips() {
    let dir = crate::common::ScratchDir::new("image-syntax");
    let path = dir.join("tree.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arena = SyntaxArena::new(&mut src, region);
    let root = alloc_syntax(&mut src, region, build_tree(&arena));
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    assert!(
        same_tree(tree_of(root), tree_of(hydrated.root)),
        "the hydrated tree differs from its source"
    );
}

// § "A span names its file by name, not by id": a `FileId` is an index into a
// process-wide interner, so the file table decides which file a hydrated span
// names. The counter-factual is the patch: renaming the spelling in the table
// renames the file every span reports. Without the file stream the hydrated
// span keeps its dumped id, which in this one process still resolves — to the
// original name, which is what this assertion refuses.
#[test]
fn a_hydrated_span_names_the_file_the_table_spells() {
    let dir = crate::common::ScratchDir::new("image-syntax-file");
    let path = dir.join("file.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arena = SyntaxArena::new(&mut src, region);
    let root = alloc_syntax(&mut src, region, Syntax::symbol(&arena, "named", at(1)));
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut bytes = std::fs::read(&path).expect("read image");
    let sections = image::sections(&bytes).expect("a freshly dumped image parses");
    let renamed = "bbb-image-syntax.lisp";
    assert_eq!(renamed.len(), FILE.len(), "the patch must not move any byte");
    let found = bytes[sections.files.clone()]
        .windows(FILE.len())
        .position(|w| w == FILE.as_bytes())
        .expect("the file table spells the span's file");
    let start = sections.files.start + found;
    bytes[start..start + renamed.len()].copy_from_slice(renamed.as_bytes());

    let source = image::ImageSource::from_bytes(&bytes).expect("anonymous file");
    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate(&mut dst, &mut SymbolTable::new(), &source).expect("hydrate");
    assert_eq!(tree_of(hydrated.root).span.file(), Some(renamed));
}

// A synthetic span names no file, and `FileId::NONE` is the absent name
// rather than an index into the table. The trap: a file stream that listed
// every span, absent ones included, would write the table's first entry over
// the absent id and give a generated node a source file it never had.
#[test]
fn a_synthetic_span_still_names_no_file() {
    let dir = crate::common::ScratchDir::new("image-syntax-synthetic");
    let path = dir.join("synthetic.image");

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arena = SyntaxArena::new(&mut src, region);
    let tree = Syntax::list(
        &arena,
        &[
            Syntax::symbol(&arena, "from-a-file", at(1)),
            Syntax::symbol(&arena, "generated", Span::synthetic()),
        ],
        Span::synthetic(),
    );
    let root = alloc_syntax(&mut src, region, tree);
    image::dump(&mut src, &SymbolTable::new(), root, &path).expect("dump");

    let mut dst = FiberHeap::new();
    let hydrated = image::hydrate_path(&mut dst, &mut SymbolTable::new(), &path).expect("hydrate");
    let kids = tree_of(hydrated.root).kind.children();
    assert_eq!(kids[0].span.file(), Some(FILE));
    assert_eq!(kids[1].span.file(), None, "a synthetic span gained a file");
    assert_eq!(
        tree_of(hydrated.root).span.file(),
        None,
        "the root's synthetic span gained a file"
    );
}

// § Dumping: two dumps of one graph are byte-identical, and a syntax node is
// the third record that has to be assembled rather than copied to make that
// true — it is a struct with a `bool` in it around an enum, so a wholesale
// copy carries its construction temporary's padding into the artifact. The
// stack painting is what makes the two temporaries differ.
#[test]
fn two_dumps_of_one_tree_are_identical() {
    let dir = crate::common::ScratchDir::new("image-syntax-determinism");
    let (a, b) = (dir.join("a.image"), dir.join("b.image"));

    let mut src = FiberHeap::new();
    let region = src.new_runtime_region();
    let arena = SyntaxArena::new(&mut src, region);
    let root = alloc_syntax(&mut src, region, build_tree(&arena));

    paint_stack(0xAA, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &a).expect("dump a");
    paint_stack(0x55, 16);
    image::dump(&mut src, &SymbolTable::new(), root, &b).expect("dump b");
    assert_eq!(
        std::fs::read(&a).expect("read a"),
        std::fs::read(&b).expect("read b"),
        "two dumps of one tree differ"
    );
}
