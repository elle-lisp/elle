// audited: 2026-10-06
//! The rest-list layout the lowerer writes onto each lambda's `LirHead`.
//!
//! docs/impl/region/restlist.md

use super::*;
use crate::value::RestListLayout;

/// The layout of the closure a `letrec` named `name`.
fn layout_of(module: &FrozenModule, name: &str) -> RestListLayout {
    module
        .closures
        .iter()
        .map(|f| f.view())
        .find(|f| f.name() == Some(name))
        .unwrap_or_else(|| panic!("no closure named {name}"))
        .rest_list_layout()
}

#[test]
fn a_fresh_head_builds_one_region_per_cell() {
    assert_eq!(
        crate::lir::LirHead::new(crate::value::Arity::AtLeast(0)).rest_list_layout,
        RestListLayout::PerCell,
        "a function no gate has judged keeps the layout that is always correct"
    );
}

/// Counterfactual: a lowerer that never reads the gate's verdict leaves every
/// lambda at the default, and `reads` comes out one region per cell.
#[test]
fn the_lowerer_writes_the_gates_verdict_onto_each_lambda() {
    let module = compile_to_lir(
        "(letrec [reads (fn [& xs] (length xs))
                  keeps (fn [& xs] xs)]
           (reads 1 2 3)
           (keeps 1 2 3))",
    );
    assert_eq!(
        layout_of(&module, "reads"),
        RestListLayout::OneRegion,
        "a lambda that only reads its rest list takes one region"
    );
    assert_eq!(
        layout_of(&module, "keeps"),
        RestListLayout::PerCell,
        "a lambda that returns its rest list keeps one region per cell"
    );
}
