// audited: 2026-10-20
//! A value read out of an env cell must outlive the reader that consumes it.
//!
//! Every shape below reads a captured binding back through its env cell and
//! hands the result to a further consumer, so a release placed at the load
//! rather than at the reader frees the value under its own reader.
//!
//! The shapes run with `--trace=scrub` armed, and that is the whole point of
//! this file. A freed page keeps its bytes, so the stale read returns the right
//! answer and every one of these programs passes on a plain build whether the
//! release is correctly placed or not. Scrub blanks a released page's body, so
//! the same read lands on an all-zero slot and `arena::deref` raises at the
//! deref site instead (docs/impl/region/diagnostics.md). The answers asserted
//! here are the ones the language semantics require, not the ones any particular
//! release placement produces.
//!
//! This file is its OWN test binary because `config::init` is process-global:
//! arming scrub inside the shared integration binary would blank pages for every
//! other test in it.
//!
//! docs/impl/region/cells.md
//! docs/impl/region/diagnostics.md

use elle::{init_stdlib, register_primitives, SymbolTable, VM};

/// Run `source` on a fresh VM and return its result rendered as a string.
fn run(source: &str) -> String {
    let mut symbols = SymbolTable::new();
    let mut vm = VM::new();
    let _ = register_primitives(&mut vm, &mut symbols);
    let mut cctx = elle::pipeline::CompileCtx::new();
    vm.set_compile_ctx(&mut cctx as *mut elle::pipeline::CompileCtx);
    vm.set_symbols(&mut symbols as *mut SymbolTable);
    init_stdlib(
        &mut vm,
        &mut symbols,
        &mut cctx,
        &elle::compiler::stdlib_cache::StdlibCache::Off,
    );
    match elle::eval_all(source, &mut symbols, &mut vm, &mut cctx, "<cell-borrow>") {
        Ok(v) => format!("{}", v.display_with(Some(&symbols))),
        Err(e) => format!("error: {e}"),
    }
}

/// `(name, one expression, the answer the semantics require)`.
///
/// The shapes differ in what stands between the capture and the read, and in
/// which node consumes the borrow — an enclosing call, a `let` binding, a
/// discarded statement. Each is one expression so it can be evaluated whole.
const SHAPES: &[(&str, &str, &str)] = &[
    (
        "read after an uncalled capturing closure",
        "((fn []
            (def vals [10 20])
            (defn peek [] (get vals 0))
            (type (get vals 0))))",
        ":integer",
    ),
    (
        "read after a called capturing closure",
        "((fn []
            (def vals [10 20])
            (defn peek [] (get vals 0))
            (peek)
            (peek)
            (type (get vals 0))))",
        ":integer",
    ),
    (
        "the borrow's consumer is a let binding",
        "((fn []
            (def vals [10 20])
            (defn peek [] vals)
            (let [x (get vals 1)] (+ x 1))))",
        "21",
    ),
    (
        "the borrow's value is discarded",
        "((fn []
            (def vals [10 20])
            (defn peek [] vals)
            (begin (get vals 0) 99)))",
        "99",
    ),
    (
        "a reader native other than get",
        "((fn []
            (def vals [10 20])
            (defn peek [] vals)
            (type (length vals))))",
        ":integer",
    ),
    (
        "the capturing closure holds a letrec",
        "((fn []
            (def vals (map (fn [p] {:check p}) [integer?]))
            (defn checker []
              (letrec [check (fn [i]
                               (when (< i 1)
                                 (let [v (get vals i)] (type v))
                                 (check (+ i 1))))]
                (check 0)))
            (checker)
            (checker)
            (type (get vals 0))))",
        ":struct",
    ),
    (
        "the read feeds a call that outlives several reads",
        "((fn []
            (def vals [1 2 3])
            (defn peek [] (get vals 0))
            (+ (get vals 0) (get vals 1) (get vals 2))))",
        "6",
    ),
    (
        "a captured param, not a captured local",
        "((fn [vals]
            (defn peek [] (get vals 0))
            (type (get vals 0))) [10 20])",
        ":integer",
    ),
];

#[test]
fn a_cell_read_outlives_its_reader() {
    let mut cfg = elle::config::Config::default();
    cfg.trace_keywords.push("scrub".to_string());
    elle::config::init(cfg);

    for (name, source, want) in SHAPES {
        let got = run(source);
        assert_eq!(
            &got, want,
            "under --trace=scrub, {name} answered {got} instead of {want} — the \
             env cell was released at the load rather than at the reader that \
             consumes the borrow, so the cell's free cascade reclaimed the \
             value under its own reader (docs/impl/region/cells.md)",
        );
    }
}
