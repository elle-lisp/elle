//! Unit tests (`super` is the parent impl module).

use crate::reader::{js_parser, lua_parser, py_parser};

// The three frontends share Nav::expect, so an unmet token expectation must
// surface the one canonical "<loc>: expected X, got Y" shape in every
// language. Each input below omits a delimiter the parser demands via
// expect(), so the error originates in the shared method — if its format
// (or the past-end Eof handling it leans on) regressed, all three break
// together and these catch it.

fn assert_located_expect_error(err: &str) {
    assert!(err.contains("expected"), "missing 'expected': {err}");
    assert!(err.contains("got"), "missing 'got': {err}");
    // "<file>:<line>:<col>: …" — a position prefix from SourceLoc::position.
    assert!(
        err.contains(':'),
        "missing location separator in error: {err}"
    );
}

#[test]
fn js_unmet_expectation_is_a_located_error() {
    // `(` with no closing `)` before end-of-input.
    let err = js_parser::parse_js_file("f(1", "t.js").unwrap_err();
    assert_located_expect_error(&err);
}

#[test]
fn py_unmet_expectation_is_a_located_error() {
    // `(` with no closing `)`.
    let err = py_parser::parse_py_file("x = (1", "t.py").unwrap_err();
    assert_located_expect_error(&err);
}

#[test]
fn lua_unmet_expectation_is_a_located_error() {
    // `if` with no `then`.
    let err = lua_parser::parse_lua_file("if x return 1 end", "t.lua").unwrap_err();
    assert_located_expect_error(&err);
}
