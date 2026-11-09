// audited: 2026-11-09
//! Tests for the epoch 12 migration: the coroutine names become fiber names,
//! and the coroutine forms with no fiber counterpart are refused.
//!
//! docs/epochs.md

use super::super::migrate;
use super::{int, list, sym};
use crate::syntax::{thread_arena, SyntaxKind};

#[test]
fn test_epoch_12_coro_new_replace() {
    // (coro/new (fn [] (yield 1))) → (fiber/new (fn [] (yield 1)) |:yield|)
    let mut forms = vec![list(vec![
        sym("coro/new"),
        list(vec![
            sym("fn"),
            list(vec![]),
            list(vec![sym("yield"), int(1)]),
        ]),
    ])];
    let count = migrate(&thread_arena(), &mut forms, 11, 12).unwrap();
    assert!(count >= 1);
    if let SyntaxKind::List(items) = &forms[0].kind {
        assert_eq!(items[0].as_symbol(), Some("fiber/new"));
        // Second arg should be the lambda
        if let SyntaxKind::List(lambda) = &items[1].kind {
            assert_eq!(lambda[0].as_symbol(), Some("fn"));
        } else {
            panic!("expected lambda as second arg");
        }
        // Third arg should be |:yield| set literal
        if let SyntaxKind::Set(elems) = &items[2].kind {
            assert_eq!(elems.len(), 1);
            assert!(matches!(&elems[0].kind, SyntaxKind::Keyword(k) if k == "yield"));
        } else {
            panic!("expected set literal |:yield|, got {:?}", items[2].kind);
        }
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_epoch_12_make_coroutine_replace() {
    // (make-coroutine f) → (fiber/new f |:yield|)
    let mut forms = vec![list(vec![sym("make-coroutine"), sym("f")])];
    let count = migrate(&thread_arena(), &mut forms, 11, 12).unwrap();
    assert!(count >= 1);
    if let SyntaxKind::List(items) = &forms[0].kind {
        assert_eq!(items[0].as_symbol(), Some("fiber/new"));
        assert_eq!(items[1].as_symbol(), Some("f"));
        if let SyntaxKind::Set(elems) = &items[2].kind {
            assert_eq!(elems.len(), 1);
            assert!(matches!(&elems[0].kind, SyntaxKind::Keyword(k) if k == "yield"));
        } else {
            panic!("expected set literal |:yield|, got {:?}", items[2].kind);
        }
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_epoch_12_coro_renames() {
    // coro/resume → fiber/resume, coro? → fiber?, etc.
    let mut forms = vec![
        list(vec![sym("coro/resume"), sym("co")]),
        list(vec![sym("coro?"), sym("x")]),
        list(vec![sym("coroutine?"), sym("x")]),
        list(vec![sym("yield-from"), sym("sub")]),
    ];
    let count = migrate(&thread_arena(), &mut forms, 11, 12).unwrap();
    assert!(count >= 4);
    if let SyntaxKind::List(items) = &forms[0].kind {
        assert_eq!(items[0].as_symbol(), Some("fiber/resume"));
    }
    if let SyntaxKind::List(items) = &forms[1].kind {
        assert_eq!(items[0].as_symbol(), Some("fiber?"));
    }
    if let SyntaxKind::List(items) = &forms[2].kind {
        assert_eq!(items[0].as_symbol(), Some("fiber?"));
    }
    if let SyntaxKind::List(items) = &forms[3].kind {
        assert_eq!(items[0].as_symbol(), Some("yield*"));
    }
}

#[test]
fn test_epoch_12_coro_iterator_removed() {
    let mut forms = vec![list(vec![sym("coro/>iterator"), sym("co")])];
    let result = migrate(&thread_arena(), &mut forms, 11, 12);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("natively iterable"));
}

#[test]
fn test_epoch_12_coroutine_next_removed() {
    let mut forms = vec![list(vec![sym("coroutine-next"), sym("co")])];
    let result = migrate(&thread_arena(), &mut forms, 11, 12);
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("fiber/resume"));
}
