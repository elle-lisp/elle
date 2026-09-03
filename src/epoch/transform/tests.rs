use super::*;
use crate::syntax::{thread_arena, Span, Syntax, SyntaxKind};

// Every `rewrite_node` test names only the rules it exercises and takes the
// rest from `Rules::none()`. Spelling all six tables out per test hid which
// one each was actually about.

fn sym(name: &str) -> Syntax {
    Syntax::symbol(&thread_arena(), name, Span::synthetic())
}

fn int(n: i64) -> Syntax {
    Syntax::new(SyntaxKind::Int(n), Span::synthetic())
}

fn list(items: Vec<Syntax>) -> Syntax {
    Syntax::list(&thread_arena(), &items, Span::synthetic())
}

#[test]
fn test_rename_symbol() {
    let rules = Rules {
        renames: [("foo", "bar")].into_iter().collect(),
        ..Rules::none()
    };

    let mut form = sym("foo");
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert_eq!(count, 1);
    assert_eq!(form.as_symbol(), Some("bar"));
}

#[test]
fn test_rename_in_list() {
    let rules = Rules {
        renames: [("old", "new")].into_iter().collect(),
        ..Rules::none()
    };

    let mut form = list(vec![sym("old"), int(1), sym("old")]);
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert_eq!(count, 2);
    if let SyntaxKind::List(items) = &form.kind {
        assert_eq!(items[0].as_symbol(), Some("new"));
        assert_eq!(items[2].as_symbol(), Some("new"));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_no_rewrite_inside_quote() {
    let rules = Rules {
        renames: [("foo", "bar")].into_iter().collect(),
        ..Rules::none()
    };

    let mut form = Syntax::new(
        SyntaxKind::Quote(thread_arena().node(sym("foo"))),
        Span::synthetic(),
    );
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert_eq!(count, 0);
    if let SyntaxKind::Quote(inner) = &form.kind {
        assert_eq!(inner.as_symbol(), Some("foo"));
    }
}

#[test]
fn test_rewrite_inside_quasiquote() {
    let rules = Rules {
        renames: [("foo", "bar")].into_iter().collect(),
        ..Rules::none()
    };

    let mut form = Syntax::new(
        SyntaxKind::Quasiquote(thread_arena().node(sym("foo"))),
        Span::synthetic(),
    );
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert_eq!(count, 1);
    if let SyntaxKind::Quasiquote(inner) = &form.kind {
        assert_eq!(inner.as_symbol(), Some("bar"));
    }
}

#[test]
fn test_removal_errors() {
    let rules = Rules {
        removals: [("gone", "use replacement instead")].into_iter().collect(),
        ..Rules::none()
    };

    let mut form = sym("gone");
    let result = rewrite_node(&thread_arena(), &mut form, &rules);

    assert!(result.is_err());
    assert!(result.unwrap_err().contains("has been removed"));
}

#[test]
fn test_no_changes_no_rules() {
    let rules = Rules::none();

    let mut form = list(vec![sym("foo"), int(1)]);
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert_eq!(count, 0);
}

#[test]
fn test_migrate_empty_range() {
    let mut forms = vec![list(vec![sym("foo"), int(1)])];
    let count = migrate(&thread_arena(), &mut forms, 0, 0).unwrap();
    assert_eq!(count, 0);
}

// --- Replace rule tests ---

#[test]
fn test_replace_basic() {
    // (assert-eq X Y msg) → (assert (= X Y) msg)
    let rules = Rules {
        replaces: vec![("assert-eq", 3usize, "(assert (= $1 $2) $3)")],
        ..Rules::none()
    };

    let mut form = list(vec![
        sym("assert-eq"),
        int(1),
        int(2),
        Syntax::string(&thread_arena(), "msg", Span::synthetic()),
    ]);
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert!(count >= 1);
    // Result should be (assert (= 1 2) "msg")
    if let SyntaxKind::List(items) = &form.kind {
        assert_eq!(items[0].as_symbol(), Some("assert"));
        if let SyntaxKind::List(inner) = &items[1].kind {
            assert_eq!(inner[0].as_symbol(), Some("="));
        } else {
            panic!("expected inner list (= ...)");
        }
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_replace_with_complex_args() {
    // (assert-eq (+ 1 2) (- 5 2) "arith") → (assert (= (+ 1 2) (- 5 2)) "arith")
    let rules = Rules {
        replaces: vec![("assert-eq", 3usize, "(assert (= $1 $2) $3)")],
        ..Rules::none()
    };

    let mut form = list(vec![
        sym("assert-eq"),
        list(vec![sym("+"), int(1), int(2)]),
        list(vec![sym("-"), int(5), int(2)]),
        Syntax::string(&thread_arena(), "arith", Span::synthetic()),
    ]);
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert!(count >= 1);
    if let SyntaxKind::List(items) = &form.kind {
        assert_eq!(items[0].as_symbol(), Some("assert"));
        if let SyntaxKind::List(eq_form) = &items[1].kind {
            assert_eq!(eq_form[0].as_symbol(), Some("="));
            // First arg should be (+ 1 2)
            if let SyntaxKind::List(plus) = &eq_form[1].kind {
                assert_eq!(plus[0].as_symbol(), Some("+"));
            } else {
                panic!("expected (+ 1 2)");
            }
        } else {
            panic!("expected (= ...)");
        }
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_replace_arity_mismatch_passthrough() {
    // (assert-eq X Y) with arity 2 should NOT match a rule expecting arity 3
    let rules = Rules {
        replaces: vec![("assert-eq", 3usize, "(assert (= $1 $2) $3)")],
        ..Rules::none()
    };

    let mut form = list(vec![sym("assert-eq"), int(1), int(2)]);
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert_eq!(count, 0);
    if let SyntaxKind::List(items) = &form.kind {
        assert_eq!(items[0].as_symbol(), Some("assert-eq"));
    }
}

#[test]
fn test_replace_and_rename_together() {
    // Replace (old-fn X Y) → (new-fn (+ $1 $2))
    // Also rename "old-sym" → "new-sym"
    // Input: (old-fn old-sym 2)
    // Expected: (new-fn (+ new-sym 2))
    let rules = Rules {
        renames: [("old-sym", "new-sym")].into_iter().collect(),
        replaces: vec![("old-fn", 2usize, "(new-fn (+ $1 $2))")],
        ..Rules::none()
    };

    let mut form = list(vec![sym("old-fn"), sym("old-sym"), int(2)]);
    let count = rewrite_node(&thread_arena(), &mut form, &rules).unwrap();

    assert!(count >= 2); // at least 1 replace + 1 rename
    if let SyntaxKind::List(items) = &form.kind {
        assert_eq!(items[0].as_symbol(), Some("new-fn"));
        if let SyntaxKind::List(inner) = &items[1].kind {
            assert_eq!(inner[0].as_symbol(), Some("+"));
            // old-sym should have been renamed to new-sym after replacement
            assert_eq!(inner[1].as_symbol(), Some("new-sym"));
        } else {
            panic!("expected inner list");
        }
    }
}

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

#[test]
fn test_epoch_10_cons_car_cdr_renames() {
    // Epoch 10 renames: cons→pair, car→first, cdr→rest
    let mut forms = vec![list(vec![
        sym("def"),
        sym("x"),
        list(vec![
            sym("cons"),
            int(1),
            list(vec![sym("cons"), int(2), sym("nil")]),
        ]),
    ])];
    let count = migrate(&thread_arena(), &mut forms, 9, 10).unwrap();
    assert!(count >= 2);
    // (def x (pair 1 (pair 2 nil)))
    if let SyntaxKind::List(items) = &forms[0].kind {
        // body of def
        if let SyntaxKind::List(pair_call) = &items[2].kind {
            assert_eq!(pair_call[0].as_symbol(), Some("pair"));
        }
    }
}
