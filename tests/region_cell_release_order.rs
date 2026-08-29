//! An env cell's box must outlive every release routed through that cell.
//!
//! A `def` inside a function body that a sibling closure captures is
//! materialized as a per-value env cell. Such a binding owes two releases at the
//! same env index: the init value's `DecrefValueRegion`, which loads the box RAW
//! and unwraps it to the content, and the box's own `DecrefCellRegion`, which
//! frees the page that unwrap reads. The box release must land at or after the
//! value release (docs/impl/region/bindings.md § "A cell's release lands at or
//! after every release routed through that cell").
//!
//! Each shape below gives the two releases different placements to reconcile:
//! the init is a CALL, so it owns a region of its own, and the capture is the
//! binding's last use, so the box's release is drawn to the capture while the
//! value's is drawn out to the enclosing form.
//!
//! The shapes run with `--trace=scrub` armed, and that is the whole point of
//! this file. A freed page keeps its bytes, so the stale unwrap returns the
//! right answer and every one of these programs passes on a plain build whether
//! the box release is correctly placed or not — the mis-ordering only detonates
//! once other work has recycled the page. Scrub blanks a released page's body,
//! so the unwrap lands on an all-zero slot and `arena::deref` raises at the
//! deref site instead (docs/impl/region/diagnostics.md).
//!
//! This file is its OWN test binary because `config::init` is process-global:
//! arming scrub inside the shared integration binary would blank pages for every
//! other test in it.

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
    match elle::eval_all(source, &mut symbols, &mut vm, &mut cctx, "<cell-release>") {
        Ok(v) => format!("{}", v.display_with(Some(&symbols))),
        Err(e) => format!("error: {e}"),
    }
}

/// `(name, one expression, the answer the semantics require)`.
const SHAPES: &[(&str, &str, &str)] = &[
    (
        "the sibling closure is never called",
        "((fn []
            (def joined (path/join \"/x\" \"b\"))
            (let [_reader (fn [] (string? joined))] :built)))",
        ":built",
    ),
    (
        "the sibling closure is called",
        "((fn []
            (def joined (path/join \"/x\" \"b\"))
            (let [reader (fn [] (string? joined))] (reader))))",
        "true",
    ),
    (
        "the enclosing form reads the def after the capture",
        "((fn []
            (def joined (path/join \"/x\" \"b\"))
            (let [_reader (fn [] (string? joined))] (string? joined))))",
        "true",
    ),
    (
        "the init is an allocating call other than a path join",
        "((fn []
            (def parts (list 10 20))
            (let [_reader (fn [] (length parts))] :built)))",
        ":built",
    ),
];

#[test]
fn a_cell_box_outlives_the_release_routed_through_it() {
    let mut cfg = elle::config::Config::default();
    cfg.trace_keywords.push("scrub".to_string());
    elle::config::init(cfg);

    for (name, source, want) in SHAPES {
        let got = run(source);
        assert_eq!(
            &got, want,
            "under --trace=scrub, {name} answered {got} instead of {want} — the \
             env cell's box was freed before the value release that unwraps it, \
             so that release read a reclaimed page (docs/impl/region/bindings.md \
             § \"A cell's release lands at or after every release routed through \
             that cell\")",
        );
    }
}
