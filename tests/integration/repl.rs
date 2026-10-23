// audited: 2026-10-21
// The REPL fed through a pipe: one form at a time, what a line binds, and the errors a session survives.
// docs/pipeline.md

use std::io::Write;
use std::path::Path;
use std::process::{Command, Stdio};

fn elle(input: &str) -> (String, String, i32) {
    elle_in(Path::new("."), input)
}

/// Run a session in `cwd`, so that a relative `import-file` path resolves
/// against it: a REPL line has no file of its own.
fn elle_in(cwd: &Path, input: &str) -> (String, String, i32) {
    let bin = env!("CARGO_BIN_EXE_elle");
    let mut child = Command::new(bin)
        .current_dir(cwd)
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .expect("failed to spawn elle");

    child
        .stdin
        .as_mut()
        .unwrap()
        .write_all(input.as_bytes())
        .unwrap();

    let out = child.wait_with_output().unwrap();
    (
        String::from_utf8_lossy(&out.stdout).to_string(),
        String::from_utf8_lossy(&out.stderr).to_string(),
        out.status.code().unwrap_or(-1),
    )
}

// ── Single form ──────────────────────────────────────────────────────

#[test]
fn single_expression() {
    let (out, _, code) = elle("(+ 1 2)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 3"), "got: {}", out);
}

// ── Multi-form on one line ───────────────────────────────────────────

#[test]
fn multi_form_one_line_shows_each_result() {
    let (out, _, code) = elle("(+ 1 2) (+ 3 4)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 3"), "missing first result: {}", out);
    assert!(out.contains("⟹ 7"), "missing second result: {}", out);
}

#[test]
fn multi_form_one_line_side_effects() {
    let (out, _, code) = elle("(print 1) (print 2) (print 3)\n");
    assert_eq!(code, 0);
    assert!(out.contains("123"), "side effects: {}", out);
}

// ── Def persistence across lines ─────────────────────────────────────

#[test]
fn def_persists_across_lines() {
    let (out, _, code) = elle("(def x 10)\n(+ x 1)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 10"), "def result: {}", out);
    assert!(out.contains("⟹ 11"), "use of def: {}", out);
}

#[test]
fn def_function_persists() {
    let (out, _, code) = elle("(def add (fn (a b) (+ a b)))\n(add 3 4)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 7"), "function call: {}", out);
}

#[test]
fn defn_persists() {
    let (out, _, code) = elle("(defn double (x) (* x 2))\n(double 5)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 10"), "defn call: {}", out);
}

#[test]
fn multiple_defs_persist() {
    let (out, _, code) = elle("(def x 10)\n(def y 20)\n(+ x y)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 30"), "sum of defs: {}", out);
}

#[test]
fn def_redefinition() {
    let (out, _, code) = elle("(def x 1)\n(def x 2)\nx\n");
    assert_eq!(code, 0);
    // Last three results: 1, 2, 2
    let lines: Vec<&str> = out.lines().filter(|l| l.contains("⟹")).collect();
    assert_eq!(lines.len(), 3, "expected 3 results: {}", out);
    assert!(lines[2].contains("2"), "redefined value: {}", out);
}

/// A function a line defines again is the body a later line's `map` runs. The
/// counter-factual recorded each line's functions for later lines to splice
/// into a fused `map`, keeping the first definition, so the `map` answered
/// `[1 1]` while a direct call answered 2.
#[test]
fn a_redefined_function_is_the_body_a_later_map_runs() {
    let (out, _, code) = elle("(defn f [x] 1)\n(defn f [x] 2)\n(map f [1 2])\n(f 0)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ [2 2]"), "the map runs the second f: {out}");
    assert!(
        out.contains("⟹ 2"),
        "a direct call runs the second f: {out}"
    );
}

// ── Multi-line accumulation ──────────────────────────────────────────

#[test]
fn multiline_expression() {
    let (out, _, code) = elle("(+ 1\n   2)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 3"), "multiline expr: {}", out);
}

#[test]
fn multiline_let() {
    let (out, _, code) = elle("(let [x 10]\n  (+ x 1))\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 11"), "multiline let: {}", out);
}

#[test]
fn multiline_defn() {
    let (out, _, code) =
        elle("(defn fib (n)\n  (if (< n 2) n\n    (+ (fib (- n 1)) (fib (- n 2)))))\n(fib 10)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 55"), "fib result: {}", out);
}

// ── Var persistence ─────────────────────────────────────────────────

#[test]
fn var_persists() {
    let (out, _, code) = elle("(var x 10)\n(+ x 1)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 11"), "var use: {}", out);
}

// ── Destructuring def persistence ────────────────────────────────────

#[test]
fn destructure_tuple_persists() {
    let (out, _, code) = elle("(def [a b] [1 2])\n(+ a b)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 3"), "destructure sum: {}", out);
}

#[test]
fn destructure_struct_persists() {
    let (out, _, code) = elle("(def {:x a :y b} {:x 10 :y 20})\n(+ a b)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 30"), "struct destructure sum: {}", out);
}

#[test]
fn var_destructure_persists() {
    let (out, _, code) = elle("(var [x y] [1 2])\n(+ x y)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 3"), "var destructure sum: {}", out);
}

// ── Macro persistence ─────────────────────────────────────────────────

#[test]
fn defmacro_persists() {
    let (out, _, code) = elle("(defmacro double (x) (list (quote *) x 2))\n(double 5)\n");
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 10"), "macro call: {}", out);
}

#[test]
fn defmacro_redefinition() {
    let (out, _, code) = elle(
        "(defmacro m (x) (list (quote +) x 1))\n(m 10)\n(defmacro m (x) (list (quote +) x 2))\n(m 10)\n",
    );
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 11"), "first def: {}", out);
    assert!(out.contains("⟹ 12"), "redefined: {}", out);
}

#[test]
fn defmacro_uses_def() {
    let (out, _, code) = elle(
        "(def offset 100)\n(defmacro add-offset (x) (list (quote +) x (quote offset)))\n(add-offset 5)\n",
    );
    assert_eq!(code, 0);
    assert!(out.contains("⟹ 105"), "macro using def: {}", out);
}

// ── Error handling ───────────────────────────────────────────────────

#[test]
fn unterminated_at_eof() {
    let (_, err, code) = elle("(+ 1\n");
    assert_eq!(code, 1);
    assert!(err.contains("unterminated"), "error msg: {}", err);
}

#[test]
fn runtime_error_exit_code() {
    let (_, _, code) = elle("(/ 1 0)\n");
    assert_eq!(code, 1);
}

#[test]
fn error_does_not_poison_session() {
    // An undefined variable error shouldn't prevent subsequent valid input
    let (out, err, _) = elle("bad-symbol\n(+ 1 2)\n");
    assert!(err.contains("undefined"), "error: {}", err);
    assert!(out.contains("⟹ 3"), "recovery: {}", out);
}

mod imports {
    include!("repl/imports.rs");
}

mod forward {
    include!("repl/forward.rs");
}
