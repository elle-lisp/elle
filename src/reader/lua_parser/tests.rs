use super::*;

#[test]
fn advance_past_eof_is_bounds_safe() {
    // Driving the cursor past the last token must not panic; it yields the
    // Eof sentinel. (The old raw `self.tokens[self.pos]` form index-panicked
    // here.)
    let mut p = LuaParser::new(vec![], "<test>", crate::syntax::thread_arena());
    assert_eq!(p.advance().token, LuaToken::Eof);
    assert_eq!(p.advance().token, LuaToken::Eof);
}

/// Parse without the prelude (for unit-testing the parser itself)
fn parse(input: &str) -> Vec<Syntax> {
    let mut lexer = LuaLexer::new(input, "<test>");
    let tokens = lexer.tokenize().expect("lex failed");
    let mut parser = LuaParser::new(tokens, "<test>", crate::syntax::thread_arena());
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
fn test_local_binding() {
    let form = parse_one("local x = 42");
    assert!(is_def(&form, "x"));
}

#[test]
fn test_function_def() {
    let form = parse_one("function add(a, b) return a + b end");
    assert!(is_def(&form, "add"));
}

#[test]
fn test_local_function() {
    let form = parse_one("local function f(x) return x end");
    assert!(is_def(&form, "f"));
}

#[test]
fn test_arithmetic() {
    let forms = parse("local x = 1 + 2 * 3");
    let form = &forms[0];
    // (def x (+ 1 (* 2 3)))
    assert!(is_def(form, "x"));
}

#[test]
fn test_if_elseif_else() {
    let forms = parse("if true then return 1 elseif false then return 2 else return 3 end");
    assert_eq!(forms.len(), 1);
    // Should be (if true 1 (if false 2 3))
    if let SyntaxKind::List(items) = &forms[0].kind {
        assert!(items[0].is_symbol("if"));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_while_loop() {
    let form = parse_one("while true do break end");
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("while"));
    } else {
        panic!("expected list");
    }
}

#[test]
fn test_table_array() {
    let form = parse_one("local t = {1, 2, 3}");
    assert!(is_def(&form, "t"));
    // The value should be ArrayMut
    if let SyntaxKind::List(items) = &form.kind {
        assert!(matches!(&items[2].kind, SyntaxKind::ArrayMut(elems) if elems.len() == 3));
    }
}

#[test]
fn test_table_struct() {
    let form = parse_one("local t = {x = 1, y = 2}");
    assert!(is_def(&form, "t"));
    if let SyntaxKind::List(items) = &form.kind {
        assert!(matches!(&items[2].kind, SyntaxKind::StructMut(elems) if elems.len() == 4));
    }
}

#[test]
fn test_string_concat() {
    let form = parse_one("local s = \"hello\" .. \" world\"");
    assert!(is_def(&form, "s"));
    // Should be (var s (string "hello" " world"))
    if let SyntaxKind::List(items) = &form.kind {
        if let SyntaxKind::List(op_items) = &items[2].kind {
            assert!(op_items[0].is_symbol("string"));
        }
    }
}

#[test]
fn test_neq() {
    let form = parse_one("local b = 1 ~= 2");
    assert!(is_def(&form, "b"));
    // Should be (def b (not (= 1 2)))
}

#[test]
fn test_field_access() {
    let forms = parse("local v = t.foo");
    let form = &forms[0];
    // (def v (get t :foo))
    assert!(is_def(form, "v"));
}

#[test]
fn test_for_loop() {
    let form = parse_one("for i = 1, 10 do print(i) end");
    // Should desugar to let + var + while
    if let SyntaxKind::List(items) = &form.kind {
        assert!(items[0].is_symbol("let"));
    } else {
        panic!("expected let form");
    }
}

#[test]
fn test_empty_file() {
    let forms = parse("");
    assert!(forms.is_empty());
}

#[test]
fn test_comment_only() {
    let forms = parse("-- just a comment\n");
    assert!(forms.is_empty());
}
