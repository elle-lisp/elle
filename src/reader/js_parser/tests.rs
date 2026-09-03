use super::*;

#[test]
fn advance_past_eof_is_bounds_safe() {
    // Driving the cursor past the last token must not panic; it yields the
    // Eof sentinel. (The old raw `self.tokens[self.pos]` form index-panicked
    // here.)
    let mut p = JsParser::new(vec![], "<test>", crate::syntax::thread_arena());
    assert_eq!(p.advance().token, JsToken::Eof);
    assert_eq!(p.advance().token, JsToken::Eof);
}

/// Parse without prelude (for unit-testing the parser itself)
fn parse(input: &str) -> Vec<Syntax> {
    let mut lexer = JsLexer::new(input, "<test>");
    let tokens = lexer.tokenize().expect("lex failed");
    let mut parser = JsParser::new(tokens, "<test>", crate::syntax::thread_arena());
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
fn test_const_binding() {
    let form = parse_one("const x = 42;");
    assert!(is_def(&form, "x"));
}

#[test]
fn test_let_binding() {
    let form = parse_one("let x = 42;");
    assert!(is_def(&form, "x"));
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("var"));
    }
}

#[test]
fn test_function_def() {
    let form = parse_one("function add(a, b) { return a + b; }");
    assert!(is_def(&form, "add"));
}

#[test]
fn test_arrow_function() {
    let form = parse_one("const f = (x) => x + 1;");
    assert!(is_def(&form, "f"));
    // The value should be (fn (x) (+ x 1))
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(fn_items) = &items[2].kind {
            assert!(fn_items[0].is_symbol("fn"));
        } else {
            panic!("expected fn form");
        }
    }
}

#[test]
fn test_arrow_single_param() {
    let form = parse_one("const f = x => x + 1;");
    assert!(is_def(&form, "f"));
}

#[test]
fn test_arrow_body_block() {
    let form = parse_one("const f = (x) => { return x + 1; };");
    assert!(is_def(&form, "f"));
}

#[test]
fn test_if_else() {
    let form = parse_one("if (x > 0) { return 1; } else { return 0; }");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("if"));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_if_else_if() {
    let form =
        parse_one("if (x > 0) { return 1; } else if (x < 0) { return -1; } else { return 0; }");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("if"));
        // else branch should be nested if
        if let SyntaxKind::List(else_items) = &items[3].kind {
            assert!(else_items[0].is_symbol("if"));
        }
    }
}

#[test]
fn test_while_loop() {
    let form = parse_one("while (x > 0) { x = x - 1; }");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("while"));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_for_of() {
    let form = parse_one("for (const x of arr) { println(x); }");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("each"));
    } else {
        panic!("expected each form");
    }
}

#[test]
fn test_for_c_style() {
    let form = parse_one("for (let i = 0; i < 10; i++) { println(i); }");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("block"));
    } else {
        panic!("expected block form");
    }
}

#[test]
fn test_arithmetic() {
    let form = parse_one("const x = 1 + 2 * 3;");
    assert!(is_def(&form, "x"));
}

#[test]
fn test_array_literal() {
    let form = parse_one("const a = [1, 2, 3];");
    assert!(is_def(&form, "a"));
    if let SyntaxKind::List(items) = &form.kind {
        assert!(matches!(&items[2].kind, SyntaxKind::ArrayMut(elems) if elems.len() == 3));
    }
}

#[test]
fn test_object_literal() {
    let form = parse_one("const o = {x: 1, y: 2};");
    assert!(is_def(&form, "o"));
    if let SyntaxKind::List(items) = &form.kind {
        assert!(matches!(&items[2].kind, SyntaxKind::StructMut(elems) if elems.len() == 4));
    }
}

#[test]
fn test_ternary() {
    let form = parse_one("const v = x > 0 ? 1 : 0;");
    assert!(is_def(&form, "v"));
    // Value should be (if (> x 0) 1 0)
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(if_items) = &items[2].kind {
            assert!(if_items[0].is_symbol("if"));
        }
    }
}

#[test]
fn test_dot_access() {
    let form = parse_one("const v = obj.field;");
    assert!(is_def(&form, "v"));
}

#[test]
fn test_index_access() {
    let form = parse_one("const v = arr[0];");
    assert!(is_def(&form, "v"));
}

#[test]
fn test_method_call() {
    let form = parse_one("obj.method(1, 2);");
    // Should be ((get obj :method) 1 2)
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(getter) = &items[0].kind {
            assert!(getter[0].is_symbol("get"));
        }
    }
}

#[test]
fn test_template_literal() {
    let form = parse_one("const s = `hello ${name}!`;");
    assert!(is_def(&form, "s"));
    // Value should be (string "hello " name "!")
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(str_items) = &items[2].kind {
            assert!(str_items[0].is_symbol("string"));
        }
    }
}

#[test]
fn test_strict_equality() {
    let forms = parse("const b = 1 === 2;");
    let form = &forms[0];
    assert!(is_def(form, "b"));
}

#[test]
fn test_not_equal() {
    let form = parse_one("const b = 1 !== 2;");
    assert!(is_def(&form, "b"));
    // Should be (def b (not (= 1 2)))
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(not_items) = &items[2].kind {
            assert!(not_items[0].is_symbol("not"));
        }
    }
}

#[test]
fn test_destructuring_array() {
    let form = parse_one("const [a, b] = pair;");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("def"));
        assert!(matches!(&items[1].kind, SyntaxKind::Array(_)));
    }
}

#[test]
fn test_rest_params() {
    let form = parse_one("function f(a, ...rest) { return rest; }");
    assert!(is_def(&form, "f"));
}

#[test]
fn test_spread_args() {
    let form = parse_one("f(...args);");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("f"));
        assert!(matches!(&items[1].kind, SyntaxKind::Splice(_)));
    }
}

#[test]
fn test_empty_file() {
    let forms = parse("");
    assert!(forms.is_empty());
}

#[test]
fn test_comment_only() {
    let forms = parse("// just a comment\n");
    assert!(forms.is_empty());
}

#[test]
fn test_shorthand_object() {
    let form = parse_one("const o = {x, y};");
    assert!(is_def(&form, "o"));
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::Struct(elems) = &items[2].kind {
            assert_eq!(elems.len(), 4); // :x x :y y
        }
    }
}

#[test]
fn test_assignment() {
    let form = parse_one("x = 42;");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("assign"));
    }
}

#[test]
fn test_field_assignment() {
    let form = parse_one("obj.x = 42;");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("put"));
    }
}

#[test]
fn test_plus_assign() {
    let form = parse_one("x += 1;");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("assign"));
    }
}
