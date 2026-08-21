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
