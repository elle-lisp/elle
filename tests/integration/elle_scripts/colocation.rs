// audited: 2026-09-29
// Guardfree pins for colocation: a value that shares a region lives while anything holds it.
//
// docs/impl/region/colocation.md

use super::*;

// Guard — the append-only container join. A pushed value is born in its
// container's region and holds no count of its own, so the region's one count
// is what keeps it. The fixture drops the container's holder while an element,
// a frozen copy, another container, a popped value or a closure still names a
// value inside it, then reads that value after region-id churn. A region freed
// early faults under guardfree. The gauge face is region-colocation.lisp.
#[test]
fn region_join_container_uaf() {
    run_elle_script_with_args(
        "region-join-container-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the one-region rest list. Every cons of a rest list shares the list's
// region, so a tail handed out of the callee keeps the whole list. The fixture
// stores, binds, applies and captures tails after the head is gone.
#[test]
fn region_rest_list_uaf() {
    run_elle_script_with_args(
        "region-rest-list-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the macro-expansion arena. Every value a transformer allocates joins
// the expansion's arena, and the close frees it once the result is copied to
// syntax. The fixture's transformers build nested templates, run closures, catch
// errors and expand again, and an `eval` loop expands macros at run time.
#[test]
fn region_macro_arena_uaf() {
    run_elle_script_with_args(
        "region-macro-arena-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
