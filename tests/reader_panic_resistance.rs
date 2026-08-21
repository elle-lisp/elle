//! The alt-language reader frontends must never *panic* on malformed input —
//! they return a clean `Ok`/`Err` instead. This guards the bounds-safety the
//! `TokenCursor` provides (its `advance()` replaced a raw `tokens[pos]` index
//! that could panic past the last token).
//!
//! Strategy: take valid JS/Python/Lua programs and feed every char-truncated
//! prefix and every single-word deletion through the parser, asserting none of
//! them unwind.

use std::panic::{catch_unwind, AssertUnwindSafe};

/// Every malformed mutation of `programs` parses without panicking.
fn assert_no_panic(name: &str, programs: &[&str]) {
    let silence = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    let mut panics: Vec<String> = Vec::new();
    let mut check = |src: &str| {
        let res = catch_unwind(AssertUnwindSafe(|| {
            let _ = elle::reader::read_syntax_all_for(src, name);
        }));
        if res.is_err() {
            panics.push(src.to_string());
        }
    };

    for &prog in programs {
        for end in 0..=prog.len() {
            if prog.is_char_boundary(end) {
                check(&prog[..end]); // truncated prefix
            }
        }
        let words: Vec<&str> = prog.split_whitespace().collect();
        for i in 0..words.len() {
            let mut w = words.clone();
            w.remove(i);
            check(&w.join(" ")); // one word deleted
        }
    }

    std::panic::set_hook(silence);
    assert!(
        panics.is_empty(),
        "{name}: parser panicked on malformed input (should be a clean Err): {panics:?}"
    );
}

#[test]
fn js_parser_never_panics_on_malformed_input() {
    assert_no_panic(
        "probe.js",
        &[
            "let x = 1;",
            "const f = (a, b) => a + b;",
            "function f(a, b) { return a + b; }",
            "if (x) { y(); } else { z(); }",
            "for (let i = 0; i < 10; i++) { print(i); }",
            "const xs = [1, 2, 3].map(x => x * 2);",
            "const o = { a: 1, b: [2, 3], c: { d: 4 } };",
            "let s = `hello ${name} world ${1 + 2}`;",
            "try { f(); } catch (e) { g(); }",
            "x.y.z(a)[b].c;",
            "a ? b : c ? d : e;",
            "while (true) { break; }",
        ],
    );
}

#[test]
fn py_parser_never_panics_on_malformed_input() {
    assert_no_panic(
        "probe.py",
        &[
            "def f(a, b):\n    return a + b\n",
            "if x:\n    y()\nelse:\n    z()\n",
            "for i in range(10):\n    print(i)\n",
            "xs = [x * 2 for x in range(5)]\n",
            "d = {'a': 1, 'b': [2, 3]}\n",
            "f = lambda x: x + 1\n",
            "a = b if c else d\n",
            "class C(Base):\n    def m(self):\n        return 1\n",
            "while True:\n    break\n",
            "x = a.b.c(1)[2]\n",
            "from mod import thing\n",
            "x = not a and b or c\n",
        ],
    );
}

#[test]
fn lua_parser_never_panics_on_malformed_input() {
    assert_no_panic(
        "probe.lua",
        &[
            "local x = 1\n",
            "function f(a, b) return a + b end\n",
            "if x then y() else z() end\n",
            "for i = 1, 10 do print(i) end\n",
            "local t = {1, 2, 3}\n",
            "local t = {a = 1, b = {2, 3}}\n",
            "while true do break end\n",
            "local s = a .. b .. c\n",
            "x = t.field\n",
            "t:method(1, 2)\n",
            "repeat x() until done\n",
            "return a, b, c\n",
        ],
    );
}
