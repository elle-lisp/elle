//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::syntax::{Span, Syntax, SyntaxKind};

fn sym(name: &str) -> Syntax {
    Syntax::new(SyntaxKind::Symbol(name.to_string()), Span::synthetic())
}

fn int(n: i64) -> Syntax {
    Syntax::new(SyntaxKind::Int(n), Span::synthetic())
}

fn list(items: Vec<Syntax>) -> Syntax {
    Syntax::new(SyntaxKind::List(items), Span::synthetic())
}

#[test]
fn test_extract_epoch_present() {
    let mut forms = vec![
        list(vec![sym("elle/epoch"), int(0)]),
        list(vec![sym("def"), sym("x"), int(10)]),
    ];

    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, Some(0));
    assert_eq!(forms.len(), 1); // (elle 0) removed
}

#[test]
fn test_extract_epoch_absent() {
    let mut forms = vec![list(vec![sym("def"), sym("x"), int(10)])];

    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None);
    assert_eq!(forms.len(), 1); // unchanged
}

#[test]
fn test_extract_epoch_empty() {
    let mut forms: Vec<Syntax> = Vec::new();
    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None);
}

#[test]
fn test_extract_epoch_negative() {
    let mut forms = vec![list(vec![sym("elle/epoch"), int(-1)])];
    let result = extract_epoch(&mut forms);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("must be non-negative"));
}

#[test]
fn test_extract_epoch_future() {
    let mut forms = vec![list(vec![sym("elle/epoch"), int(CURRENT_EPOCH as i64 + 1)])];
    let result = extract_epoch(&mut forms);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("only supports up to"));
}

#[test]
fn test_extract_epoch_not_elle() {
    let mut forms = vec![list(vec![sym("notelle"), int(0)])];
    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None);
    assert_eq!(forms.len(), 1);
}

#[test]
fn test_extract_epoch_wrong_arity() {
    let mut forms = vec![list(vec![sym("elle/epoch")])];
    let epoch = extract_epoch(&mut forms).unwrap();
    assert_eq!(epoch, None); // not recognized, left alone
}

#[test]
fn test_migrate_forms_current_epoch() {
    let mut forms = vec![list(vec![sym("foo"), int(1)])];
    let count = migrate_forms(&mut forms, CURRENT_EPOCH).unwrap();
    assert_eq!(count, 0);
}
