// audited: 2026-10-21
// The names a code payload holds outside its values, and a fresh instance
// that can spell them after hydration.
// docs/impl/region/template.md

use super::*;
use elle::image;

/// A function whose fragment names its own name, a parameter, a `let` binding
/// and a top-level global, none of which a value in its graph spells. `%pair`
/// builds at run time, so the global is a real capture rather than a constant
/// the lowerer inlines.
const GRAFTED: &str = "(def image-frag-global (%pair 40 2))\n\
                       (defn image-frag-fn [image-frag-param]\n\
                         (let [image-frag-local (%pair image-frag-param image-frag-global)]\n\
                           image-frag-local))\n\
                       image-frag-fn";

/// A function whose body mints a compiled capture cell. Inside a lambda only
/// a local function a sibling captures gets one, so the body holds two that
/// call each other. The body holds lambdas, so it closes into no fragment, and
/// the cell's name lives only in the LIR and the bytecode.
const CELLED: &str = "(defn image-cell-fn []\n\
                        (defn image-cell-name [n] (if (%eq n 0) 0 (image-cell-other n)))\n\
                        (defn image-cell-other [n] (image-cell-name 0))\n\
                        image-cell-name)\n\
                      image-cell-fn";

/// Assert that `names` spells every one of `want`.
fn spells(names: &SymbolTable, want: &[&str]) {
    for &name in want {
        assert_eq!(
            names.name(SymbolId::of(name)),
            Some(name),
            "the hydrating instance cannot spell {name}, which the payload names"
        );
    }
}

// A hydrated function's fragment names its bindings by hash, and a graft of
// it mints bindings under those names. The counter-factual is the fragment as
// opaque bytes: the function hydrates and grafts correctly, and an escape dump
// or an error then names each grafted binding `~`.
#[test]
fn a_fragments_names_reach_a_fresh_instance() {
    let dir = crate::common::ScratchDir::new("image-name-fragment");
    let path = dir.join("fragment.image");

    let (mut rt, f) = compiled(GRAFTED, "<image-names>");
    assert!(
        !closure_of(f).template.fragment().is_empty(),
        "the function closes into no fragment, so this test exercises nothing"
    );
    dump_from(&mut rt, f, &path);

    let mut dst = traited_heap();
    let mut dst_names = SymbolTable::new();
    image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    spells(
        &dst_names,
        &[
            "image-frag-fn",
            "image-frag-param",
            "image-frag-local",
            "image-frag-global",
        ],
    );
}

// A capture cell's name is a raw id in the LIR and in the bytecode, and the
// cell the function mints at run time carries it. The counter-factual is a
// dump that spells only the values it copies: the cell's name never reaches
// the hydrating instance.
#[test]
fn a_capture_cells_name_reaches_a_fresh_instance() {
    use elle::lir::InstrRef;

    let dir = crate::common::ScratchDir::new("image-name-cell");
    let path = dir.join("cell.image");

    let (mut rt, f) = compiled(CELLED, "<image-names>");
    let names_the_cell = closure_of(f)
        .template
        .lir()
        .expect("a compiled lambda carries LIR")
        .nodes()
        .any(|n| {
            matches!(n.instr(), InstrRef::MakeCaptureCell { name, .. }
                     if name == SymbolId::of("image-cell-name"))
        });
    assert!(
        names_the_cell,
        "the function mints no named cell, so this test exercises nothing"
    );
    dump_from(&mut rt, f, &path);

    let mut dst = traited_heap();
    let mut dst_names = SymbolTable::new();
    image::hydrate_path(&mut dst, &mut dst_names, &path).expect("hydrate");
    spells(&dst_names, &["image-cell-name"]);
}
