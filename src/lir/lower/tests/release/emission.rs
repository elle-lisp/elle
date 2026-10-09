// audited: 2026-10-06
//! That a release is emitted at all, and at the `decref_point` the walk chose.
//!
//! docs/impl/region/rules.md

use super::*;

#[test]
fn decref_region_emitted_for_one_alloc_let() {
    // Under unique-per-alloc the lowerer emits one `DecrefRegion`
    // per region at each region's `decref_point` HirId. The walk also
    // registers regions for `Let`/`Letrec`/`Begin`/`Match`/`Call`
    // nodes (for capture-cell and per-call bookkeeping), so the
    // total count is more than just the one user-visible allocation.
    // Assert there is at least one DecrefRegion, that is, the emission path
    // is wired (there would be zero if `emit_decrefs_for` were not called).
    let module = compile_to_lir("(fn () (let [x (string \"a\")] x))");
    assert!(
        count_decref_regions(&module) >= 1,
        "expected at least one DecrefRegion to be emitted by emit_decrefs_for",
    );
}

#[test]
fn decref_region_emitted_for_emit_yield() {
    // `(fn () (let [x (string "a")] (emit :yield x)))` — the yielded
    // value's region is decref'd at the Emit's HirId (the value's
    // last use); the runtime incref in `handle_emit`
    // keeps the region alive past the matching DecrefRegion at the
    // resume site.
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    assert!(
        count_decref_regions(&module) >= 1,
        "expected at least one DecrefRegion for the emit-yielded value",
    );
}

#[test]
fn release_emitted_for_unbound_call_result() {
    // An unbound Call result — `(f "a")` whose result flows
    // directly into Begin's discard position — must have a
    // DecrefValueRegion emitted at its decref_point. Without this,
    // the call's result region survives until fiber teardown
    // (linear leak in loops). `emit_decrefs_for` releases it by value
    // off the freshly-lowered result register, the discarded-result
    // route in `emit_decref_for_region`.
    let module = compile_to_lir("(fn () (begin (f \"a\" \"b\") nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the unbound (f ...) result",
    );
}

#[test]
fn release_emitted_for_let_bound_call_result() {
    // The let-bound Call result path produces a DecrefValueRegion too: the
    // binding's slot is the release route `lower_let` records.
    let module = compile_to_lir("(fn () (let [x (f \"a\" \"b\")] nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the let-bound (f ...) result",
    );
}

#[test]
fn release_emitted_for_discarded_let_tail_call_result() {
    // `(begin (let [x 1] (f "a")) nil)` — the discarded Let's body TAIL call.
    // ANF's propagating-tail wrap keys the slot recording on the outer
    // Let's id, not the tail Call's, so the call-result placeholder
    // reaches its decref_point (the Call node itself) with no slot bound.
    // The release must then be emitted by VALUE off the freshly-lowered
    // result register (docs/impl/region/rules.md Rule 2, "discarded result").
    // The counter-factual: a lowerer that skips it leaks one object per loop
    // iteration until fiber teardown, the tests/impl/arena-count.lisp class.
    let module = compile_to_lir("(fn () (begin (let [x 1] (f \"a\")) nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected a DecrefValueRegion for the discarded let-tail (f ...) result",
    );
}

#[test]
fn named_param_release_follows_destructure_field_reads() {
    // `(fn [&named frame] 42)` compiles a prologue of
    // `(destructure {:frame frame} (var __named_param))`. The collected
    // keyword struct's region must be released AFTER the destructure's
    // field reads (`StructGetOrNil`) — the Destructure node extends the
    // value's regions' decref_point to itself (docs/impl/region/rules.md Rule 4),
    // exactly as Return extends a returned region. The counter-factual: with
    // `frame` unused, the struct's last USE is the inner Var, so the
    // `DecrefValueRegion` lands before the field read — a freed-page read at
    // runtime (tests/impl/region-named-param-uaf.lisp).
    let module = compile_to_lir("(fn [&named frame] 42)");
    let mut checked = false;
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let instrs = flat_instrs(func);
        let last_get = instrs
            .iter()
            .rposition(|i| matches!(i, InstrRef::StructGetOrNil { .. }));
        let first_decref = instrs
            .iter()
            .position(|i| matches!(i, InstrRef::DecrefValueRegion { .. }));
        if let (Some(get), Some(dec)) = (last_get, first_decref) {
            checked = true;
            assert!(
                dec > get,
                "the &named collected struct's DecrefValueRegion (idx {dec}) must \
                 follow the destructure's StructGetOrNil field reads (last idx {get})"
            );
        }
    }
    assert!(
        checked,
        "expected a function with both StructGetOrNil and DecrefValueRegion \
         (the &named prologue)"
    );
}

#[test]
fn release_emitted_for_eval_result() {
    // `(fn () (begin (eval 1) nil))` — the Eval's result is
    // discarded. Eval's result region is a placeholder in the
    // outer compilation (the actual value lives in the inner
    // compilation's region). The regions walk registers Eval's
    // placeholder in `call_result_regions`, mirroring Call, so
    // `emit_decrefs_for` releases it by value (`DecrefValueRegion`) at
    // the Eval's decref_point, and the release follows the value's
    // actual runtime region.
    //
    // The counter-factual: without this wiring, the walk's `alloc_here` for
    // Eval's HirId would land in the else branch of
    // `emit_decrefs_for`, which emits raw `DecrefRegion(rid)` for
    // a region the runtime never allocated into — counter
    // underflow or conflation with a neighbouring region id.
    let module = compile_to_lir("(fn () (begin (eval 1) nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the (eval ...) result",
    );
}

#[test]
#[ignore = "sibling merging is not implemented (docs/impl/region/merging.md)"]
fn decref_region_emitted_once_for_merged_pair() {
    // `(let [x (string "a") y (string "b")] (g x y))` has two
    // allocations with identical decref_point and no cross-region
    // edges. A sibling merge would collapse them into one region, and the
    // lowerer would emit exactly one `DecrefRegion` for the merged group.
    let module = compile_to_lir("(let [x (string \"a\") y (string \"b\")] (g x y))");
    assert_eq!(
        count_decref_regions(&module),
        1,
        "merged x and y should share one DecrefRegion",
    );
}

// The native-tail ReturnValue retain (the `IncrefValueRegion` the post-
// `TailCall` block emits on the native-completion fall-through) is guarded by
// Elle tests, not a Rust LIR-structural assertion: the non-splice path by
// tests/impl/region-native-tail-return-uaf.lisp and the splice/`apply` path by
// tests/impl/region-splice-tail-return.lisp, each a UAF witness under
// `--trace=guardfree`.
