// audited: 2026-10-13
//! What the standard library's compile records into the cross-unit registries.
//!
//! docs/impl/dissolution/inline.md

use crate::pipeline::{compile_file, CompileCtx};
use crate::symbol::SymbolTable;
use crate::value::SymbolId;

/// Compile `src` as the standard library's compile is run, and answer the
/// names each registry then holds: the dispatch registry's, then the inline
/// registry's.
fn recorded(src: &str) -> (Vec<SymbolId>, Vec<SymbolId>) {
    let mut symbols = SymbolTable::new();
    let mut cctx = CompileCtx::new();
    cctx.compiling_stdlib(|cctx| compile_file(src, &mut symbols, cctx, "<stdlib-shaped>"))
        .expect("compiles");
    let (dispatch, inline) = cctx.stdlib_registries_mut();
    (
        dispatch.by_name.keys().copied().collect(),
        inline.by_name.keys().copied().collect(),
    )
}

/// A top-level function records its body, and a function a body binds does
/// not. The counter-factual recorded every `let`-bound lambda under its name,
/// as it recorded `import/resolve`'s `file-in`.
#[test]
fn only_a_top_level_function_records_its_body() {
    let (_, inline) = recorded(
        "(defn top [x] x)
         (defn outer [] (let [inner (fn [x] x)] inner))
         (fn [] {})",
    );
    assert!(
        inline.contains(&SymbolId::of("top")),
        "a top-level function records, or this test proves nothing"
    );
    assert!(
        !inline.contains(&SymbolId::of("inner")),
        "a function a body binds is local to that body"
    );
}

/// A top-level dispatch wrapper records its arms, and a wrapper a body binds
/// does not. The counter-factual recorded the nested wrapper under its name.
#[test]
fn only_a_top_level_wrapper_records_its_arms() {
    let (dispatch, _) = recorded(
        "(defn top [c] (match (type-of c) :array (string c) _ 0))
         (defn outer []
           (let [inner (fn [c] (match (type-of c) :array (string c) _ 0))] inner))
         (fn [] {})",
    );
    assert!(
        dispatch.contains(&SymbolId::of("top")),
        "a top-level wrapper records, or this test proves nothing"
    );
    assert!(
        !dispatch.contains(&SymbolId::of("inner")),
        "a wrapper a body binds is local to that body"
    );
}
