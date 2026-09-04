//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn fileless_span_converts_to_an_unknown_source_loc() {
    // A span with no file must round-trip to a SourceLoc that SourceLoc's
    // own is_unknown() recognises. Both sides now reference the single
    // reader::UNKNOWN_FILE const, so the conversion and the predicate can't
    // disagree about what "unknown origin" spells.
    assert!(Span::synthetic().to_source_loc().is_unknown());
    assert!(Span::new(0, 1, 2, 3).to_source_loc().is_unknown());
    // A span that does carry a file is not unknown.
    assert!(!Span::new(0, 1, 2, 3)
        .with_file("m.elle")
        .to_source_loc()
        .is_unknown());
}

#[test]
fn a_span_is_copy_pod_with_no_rust_heap_allocation() {
    // The claim the region-native tree rests on: a span is bytes. It rides
    // inside a region-resident node, so a `String` field would put a Rust
    // heap allocation inside a region page — the thing the migration
    // removes (docs/impl/syntax.md § "Span").
    fn assert_copy<T: Copy>() {}
    assert_copy::<Span>();
    assert!(!std::mem::needs_drop::<Span>());
    assert_eq!(std::mem::size_of::<Span>(), 20);
}

#[test]
fn a_span_serializes_its_file_name_not_its_id() {
    // A FileId means nothing in another process, so the wire form must carry
    // the spelling. The counter-factual this pins: deriving Serialize on the
    // packed struct would ship the index, and a receiver whose table minted
    // ids in a different order would decode a span pointing at the wrong
    // file — or at no file at all.
    let span = Span::new(3, 9, 4, 2).with_file("span-serde.lisp");
    let bytes = bincode::serialize(&span).unwrap();
    let text = String::from_utf8_lossy(&bytes);
    assert!(
        text.contains("span-serde.lisp"),
        "the encoded span must contain the file name, got {:?}",
        text
    );

    let back: Span = bincode::deserialize(&bytes).unwrap();
    assert_eq!(back.file(), Some("span-serde.lisp"));
    assert_eq!((back.start, back.end, back.line, back.col), (3, 9, 4, 2));
}

#[test]
fn a_fileless_span_round_trips_as_fileless() {
    let bytes = bincode::serialize(&Span::new(1, 2, 3, 4)).unwrap();
    let back: Span = bincode::deserialize(&bytes).unwrap();
    assert_eq!(back.file(), None);
    assert_eq!(back.file_id(), FileId::NONE);
}

#[test]
fn merge_keeps_the_first_file_it_has() {
    let a = Span::new(0, 5, 1, 1).with_file("a.lisp");
    let b = Span::new(10, 15, 2, 5).with_file("b.lisp");
    assert_eq!(a.merge(&b).file(), Some("a.lisp"));
    // A fileless left operand takes the right operand's file, as the owned
    // `Option::or_else` form did.
    assert_eq!(Span::new(0, 5, 1, 1).merge(&b).file(), Some("b.lisp"));
}
