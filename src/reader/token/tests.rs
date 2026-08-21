//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn file_defaulting_constructors_agree_with_is_unknown() {
    // The defect this guards against: the constructors that default the
    // file and the predicate that tests for it used to spell the sentinel
    // literally in four separate places. If any one drifts, a location
    // built as "unknown" would no longer report as unknown.
    assert!(SourceLoc::from_line_col(3, 7).is_unknown());
    assert!(SourceLoc::start().is_unknown());
    assert_eq!(SourceLoc::from_line_col(3, 7).file, UNKNOWN_FILE);
}

#[test]
fn a_named_file_is_not_unknown() {
    assert!(!SourceLoc::new("main.elle", 1, 1).is_unknown());
    // ...and with_file can flip a location back to a known origin.
    assert!(!SourceLoc::start().with_file("main.elle").is_unknown());
}

#[test]
fn display_matches_position() {
    // Display and position() must render the same "file:line:col" form;
    // Display now delegates to position() so the two cannot diverge.
    let loc = SourceLoc::new("main.elle", 12, 4);
    assert_eq!(loc.to_string(), loc.position());
    assert_eq!(loc.to_string(), "main.elle:12:4");
}

// The macro guarantees Token and OwnedToken share a variant set; these
// tests pin the From conversion for each of the three variant groups so a
// wrong edit to the macro body (e.g. forgetting `.to_string()` on a
// borrowed arm, or swapping payloads) is caught rather than silently
// mistranslating a token.

#[test]
fn from_carries_unit_variants_through_unchanged() {
    assert_eq!(OwnedToken::from(Token::LeftParen), OwnedToken::LeftParen);
    assert_eq!(OwnedToken::from(Token::AtPipe), OwnedToken::AtPipe);
    assert_eq!(OwnedToken::from(Token::Nil), OwnedToken::Nil);
}

#[test]
fn from_owns_borrowed_string_payloads() {
    // Symbol/Keyword borrow in Token and must become owned Strings.
    assert_eq!(
        OwnedToken::from(Token::Symbol("foo")),
        OwnedToken::Symbol("foo".to_string())
    );
    assert_eq!(
        OwnedToken::from(Token::Keyword("bar")),
        OwnedToken::Keyword("bar".to_string())
    );
}

#[test]
fn from_preserves_owned_payloads_by_value() {
    assert_eq!(
        OwnedToken::from(Token::Integer(-7)),
        OwnedToken::Integer(-7)
    );
    assert_eq!(OwnedToken::from(Token::Float(2.5)), OwnedToken::Float(2.5));
    assert_eq!(OwnedToken::from(Token::Bool(true)), OwnedToken::Bool(true));
    assert_eq!(
        OwnedToken::from(Token::String("hi".to_string())),
        OwnedToken::String("hi".to_string())
    );
    assert_eq!(
        OwnedToken::from(Token::Comment("; c".to_string())),
        OwnedToken::Comment("; c".to_string())
    );
}
