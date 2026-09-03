use super::*;

#[test]
fn advance_past_eof_is_bounds_safe() {
    // Driving the cursor past the last token must not panic; it yields the
    // Eof sentinel. (The old raw `self.tokens[self.pos]` form index-panicked
    // here.)
    let mut p = PyParser::new(vec![], "<test>", crate::syntax::thread_arena());
    assert_eq!(p.advance().token, PyToken::Eof);
    assert_eq!(p.advance().token, PyToken::Eof);
}

fn parse(input: &str) -> Vec<Syntax> {
    let mut lexer = PyLexer::new(input, "<test>");
    let tokens = lexer.tokenize().expect("lex failed");
    let mut parser = PyParser::new(tokens, "<test>", crate::syntax::thread_arena());
    parser.parse_file().expect("parse failed")
}

fn parse_one(input: &str) -> Syntax {
    let mut forms = parse(input);
    assert_eq!(forms.len(), 1, "expected 1 form, got {}", forms.len());
    forms.pop().unwrap()
}

fn is_def(s: &Syntax, name: &str) -> bool {
    if let SyntaxKind::List(items) = &s.kind {
        items.len() == 3
            && (items[0].is_symbol("def") || items[0].is_symbol("var"))
            && items[1].is_symbol(name)
    } else {
        false
    }
}

#[test]
fn test_assignment() {
    let form = parse_one("x = 42\n");
    assert!(is_def(&form, "x"));
}

#[test]
fn test_function_def() {
    let form = parse_one("def add(a, b):\n  return a + b\n");
    assert!(is_def(&form, "add"));
}

#[test]
fn test_lambda() {
    let form = parse_one("f = lambda x: x + 1\n");
    assert!(is_def(&form, "f"));
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(fn_items) = &items[2].kind {
            assert!(fn_items[0].is_symbol("fn"));
        } else {
            panic!("expected fn form");
        }
    }
}

#[test]
fn test_if_elif_else() {
    let form = parse_one("if x > 0:\n  y = 1\nelif x < 0:\n  y = -1\nelse:\n  y = 0\n");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("if"));
        // else branch should be nested if (from elif)
        if let SyntaxKind::List(else_items) = &items[3].kind {
            assert!(else_items[0].is_symbol("if"));
        }
    }
}

#[test]
fn test_while_loop() {
    let form = parse_one("while x > 0:\n  x = x - 1\n");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("while"));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_for_loop() {
    let form = parse_one("for x in arr:\n  println(x)\n");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("each"));
    } else {
        panic!("expected each form");
    }
}

#[test]
fn test_arithmetic() {
    let form = parse_one("x = 1 + 2 * 3\n");
    assert!(is_def(&form, "x"));
}

#[test]
fn test_list_literal() {
    let form = parse_one("a = [1, 2, 3]\n");
    assert!(is_def(&form, "a"));
    if let SyntaxKind::List(items) = &form.kind {
        assert!(matches!(&items[2].kind, SyntaxKind::ArrayMut(elems) if elems.len() == 3));
    }
}

#[test]
fn test_dict_literal() {
    let form = parse_one("d = {\"x\": 1, \"y\": 2}\n");
    assert!(is_def(&form, "d"));
    if let SyntaxKind::List(items) = &form.kind {
        assert!(matches!(&items[2].kind, SyntaxKind::StructMut(elems) if elems.len() == 4));
    }
}

#[test]
fn test_dot_access() {
    let form = parse_one("v = obj.field\n");
    assert!(is_def(&form, "v"));
}

#[test]
fn test_index_access() {
    let form = parse_one("v = arr[0]\n");
    assert!(is_def(&form, "v"));
}

#[test]
fn test_ternary() {
    let form = parse_one("v = 1 if x > 0 else 0\n");
    assert!(is_def(&form, "v"));
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(if_items) = &items[2].kind {
            assert!(if_items[0].is_symbol("if"));
        }
    }
}

#[test]
fn test_not_equal() {
    let form = parse_one("b = 1 != 2\n");
    assert!(is_def(&form, "b"));
}

#[test]
fn test_rest_params() {
    let form = parse_one("def f(a, *args):\n  return a\n");
    assert!(is_def(&form, "f"));
}

#[test]
fn test_empty_file() {
    let forms = parse("");
    assert!(forms.is_empty());
}

#[test]
fn test_comment_only() {
    let forms = parse("# just a comment\n");
    assert!(forms.is_empty());
}

#[test]
fn test_pass() {
    let form = parse_one("def f():\n  pass\n");
    assert!(is_def(&form, "f"));
}

#[test]
fn test_string_concat() {
    let form = parse_one("s = \"hello\" \"world\"\n");
    assert!(is_def(&form, "s"));
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::String(s) = &items[2].kind {
            assert_eq!(s, "helloworld");
        }
    }
}

#[test]
fn test_field_assignment() {
    let form = parse_one("obj.x = 42\n");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("put"));
    }
}

#[test]
fn test_plus_assign() {
    let form = parse_one("x += 1\n");
    // x is not a new binding, it's an existing var — but since we see it
    // as a compound assignment, we emit (assign x (+ x 1))
    // Actually the parser sees x as an expr, then +=, so it emits assign
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("assign"));
    }
}

#[test]
fn test_boolean_ops() {
    let form = parse_one("v = a and b or not c\n");
    assert!(is_def(&form, "v"));
}

#[test]
fn test_power() {
    let form = parse_one("v = 2 ** 10\n");
    assert!(is_def(&form, "v"));
}

#[test]
fn test_for_with_assign_and_break() {
    let forms = parse("found = None\nfor i in [1, 2, 3]:\n  found = i\n  if i > 1:\n    break\n\nprintln(found)\n");
    // The for body should use `begin` (not `block`) and `assign` (not `var`)
    let for_form = &forms[1];
    if let SyntaxKind::List(items) = &for_form.kind {
        assert!(items[0].is_symbol("each"));
        let body = &items[4];
        if let SyntaxKind::List(body_items) = &body.kind {
            assert!(
                body_items[0].is_symbol("begin"),
                "for body should use begin, got {:?}",
                body_items[0]
            );
            if let SyntaxKind::List(assign_items) = &body_items[1].kind {
                assert!(
                    assign_items[0].is_symbol("assign"),
                    "should use assign inside for, got {:?}",
                    assign_items[0]
                );
            }
        }
    }
}
