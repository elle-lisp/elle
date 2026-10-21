// audited: 2026-10-21
//! The names a closure's code payload holds outside its values cross in the bundle's name table.
//!
//! docs/impl/region/template.md

use super::*;

/// Compile `source` in a fresh runtime and send its last form's value under
/// the runtime's memo, answering the bundle.
fn sent(source: &str) -> SendBundle {
    let mut rt = crate::runtime::Runtime::new();
    let value = {
        let (vm, symbols, cctx) = rt.parts();
        crate::pipeline::eval_all(source, symbols, vm, cctx, "<send-names>").expect("eval")
    };
    let (heap, symbols) = rt.heap_and_symbols();
    SendBundle::from_value(value, heap, Some(symbols)).expect("the value is sendable")
}

/// Assert that `bundle`'s name table spells every one of `want`.
fn bundle_spells(bundle: &SendBundle, want: &[&str]) {
    for &name in want {
        let id = crate::value::SymbolId::of(name).0;
        assert!(
            bundle
                .symbols
                .iter()
                .any(|(hash, spelled)| *hash == id && &**spelled == name),
            "the bundle does not spell {name}, which the payload names"
        );
    }
}

// A closure's fragment names its bindings by hash, inside bytes no value walk
// reads, and a graft in the receiving instance mints bindings under those
// names. The counter-factual is the fragment crossing as opaque bytes: the
// closure arrives and grafts, and the receiver names each binding `~`.
#[test]
fn a_closures_fragment_names_cross_in_the_bundle() {
    let bundle = sent(
        "(def send-frag-global (%pair 40 2))\n\
         (defn send-frag-fn [send-frag-param]\n\
           (let [send-frag-local (%pair send-frag-param send-frag-global)]\n\
             send-frag-local))\n\
         send-frag-fn",
    );
    assert!(
        bundle.closures.iter().any(|c| !c.fragment.is_empty()),
        "no sent closure carries a fragment, so this test exercises nothing"
    );
    bundle_spells(
        &bundle,
        &[
            "send-frag-fn",
            "send-frag-param",
            "send-frag-local",
            "send-frag-global",
        ],
    );
}

// A capture cell's name is a raw id in the LIR and in the bytecode, and the
// cell the closure mints in the receiving instance carries it. Inside a lambda
// only a local function a sibling captures gets a compiled cell, so the body
// holds two that call each other. The counter-factual is a bundle that spells
// only the values it walks.
#[test]
fn a_capture_cells_name_crosses_in_the_bundle() {
    use crate::lir::{InstrRef, LirView};

    let bundle = sent(
        "(defn send-cell-fn []\n\
           (defn send-cell-name [n] (if (%eq n 0) 0 (send-cell-other n)))\n\
           (defn send-cell-other [n] (send-cell-name 0))\n\
           send-cell-name)\n\
         send-cell-fn",
    );
    let cell = crate::value::SymbolId::of("send-cell-name");
    let names_the_cell = bundle
        .closures
        .iter()
        .filter_map(|c| c.lir.as_ref())
        .any(|code| {
            LirView::over(code, &[]).nodes().any(
                |n| matches!(n.instr(), InstrRef::MakeCaptureCell { name, .. } if name == cell),
            )
        });
    assert!(
        names_the_cell,
        "no sent closure mints the named cell, so this test exercises nothing"
    );
    bundle_spells(&bundle, &["send-cell-name"]);
}
