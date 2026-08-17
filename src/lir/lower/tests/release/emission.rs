use super::*;

// ── Region-lifecycle: decref/release emission ────────────────────

#[test]
fn decref_region_emitted_for_one_alloc_let() {
    // Under unique-per-alloc the lowerer emits one `DecrefRegion`
    // per region at each region's `decref_point` HirId. The walk also
    // registers regions for `Let`/`Letrec`/`Begin`/`Match`/`Call`
    // nodes (for capture-cell and per-call bookkeeping), so the
    // total count is more than just the one user-visible allocation.
    // Assert there's at least one DecrefRegion — i.e. the new
    // emission path is wired (we'd see zero if `emit_decrefs_for`
    // weren't called).
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

/// The `(retains, releases)` a module's park is wrapped in — retains in the block
/// the `Emit` terminates, releases in the resume block it jumps to. Counted per
/// block rather than per function so an unrelated mint elsewhere in the body (a
/// `Return`'s, say) cannot stand in for the park's own pair.
fn park_borrow_ops(module: &crate::lir::LirModule) -> (usize, usize) {
    fn in_func(func: &LirFunction) -> Option<(usize, usize)> {
        let (park, resume_label) =
            func.blocks
                .iter()
                .find_map(|b| match b.terminator.terminator {
                    crate::lir::Terminator::Emit { resume_label, .. } => Some((b, resume_label)),
                    _ => None,
                })?;
        let resume = func.blocks.iter().find(|b| b.label == resume_label)?;
        let count = |b: &crate::lir::BasicBlock, f: fn(&LirInstr) -> bool| {
            b.instructions.iter().filter(|i| f(&i.instr)).count()
        };
        Some((
            count(park, |i| matches!(i, LirInstr::IncrefValueRegion { .. })),
            count(resume, |i| matches!(i, LirInstr::DecrefValueRegion { .. })),
        ))
    }
    in_func(&module.entry)
        .or_else(|| module.closures.iter().find_map(in_func))
        .expect("a function terminating in Emit")
}

#[test]
fn park_mints_a_body_reference_for_a_borrowed_payload() {
    // The yielding lambda closes over a value the ENCLOSING lambda allocates and
    // releases, so it owns no reference to strand in the continuation the discard
    // discharge stands in for. `lower_emit` mints one: a retain before the
    // suspend and a release first in the resume block
    // (docs/impl/region/owner.md § "Park/unpark symmetry").
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (fn () (emit :yield x))))");
    let (retains, releases) = park_borrow_ops(&module);
    assert!(
        retains >= 1 && releases >= 1,
        "a borrowed yield payload must be retained across the park and released \
         at the resume; got {retains} retain(s) and {releases} release(s)",
    );
}

#[test]
fn park_mints_nothing_for_a_body_allocated_payload() {
    // The contrast: the body allocates what it yields, so its own release is
    // already the one the discharge stands in for. A second reference here would
    // be stranded at every abandoned park.
    let module = compile_to_lir("(fn () (let [x (string \"a\")] (emit :yield x)))");
    let (retains, _) = park_borrow_ops(&module);
    assert_eq!(
        retains, 0,
        "a body-allocated yield payload already carries the body's reference; \
         minting a second strands it per abandoned park",
    );
}

#[test]
fn release_emitted_for_unbound_call_result() {
    // An unbound Call result — `(f "a")` whose result flows
    // directly into Begin's discard position — must have a
    // DecrefValueRegion emitted at its decref_point. Without this,
    // the call's result region survives until fiber teardown
    // (linear leak in loops). `lower_call` allocates a release
    // slot for every Call so emit_decrefs_for can emit
    // `LoadLocal slot` + `DecrefValueRegion` uniformly for
    // both bound and unbound Calls.
    let module = compile_to_lir("(fn () (begin (f \"a\" \"b\") nil))");
    assert!(
        count_decref_value_regions(&module) >= 1,
        "expected at least one DecrefValueRegion for the unbound (f ...) result",
    );
}

#[test]
fn release_emitted_for_let_bound_call_result() {
    // Sanity check: the existing let-bound Call result path
    // also produces a DecrefValueRegion. This guards against
    // a regression where removing the redundant call_region_slot
    // recording in lower_let breaks the bound case.
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
    // result register (docs/impl/region/rules.md Rule 2, "discarded result") —
    // before that rule the lowerer skipped it ("leak until fiber
    // teardown"): one leaked object per loop iteration, the
    // tests/elle/arena-count.lisp class.
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
    // exactly as Return extends a returned region. Pre-fix, with `frame`
    // unused, the struct's last USE was the inner Var, so the
    // `DecrefValueRegion` was emitted before the field read — a freed-page
    // read at runtime (tests/elle/region-named-param-uaf.lisp, the
    // lib/http2/stream.lisp import segv).
    let module = compile_to_lir("(fn [&named frame] 42)");
    let mut checked = false;
    for func in std::iter::once(&module.entry).chain(module.closures.iter()) {
        let instrs: Vec<&LirInstr> = func
            .blocks
            .iter()
            .flat_map(|b| b.instructions.iter())
            .map(|si| &si.instr)
            .collect();
        let last_get = instrs
            .iter()
            .rposition(|i| matches!(i, LirInstr::StructGetOrNil { .. }));
        let first_decref = instrs
            .iter()
            .position(|i| matches!(i, LirInstr::DecrefValueRegion { .. }));
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
    // placeholder in `call_result_regions`, mirroring Call, and
    // `lower_eval` wraps the result with
    // `wrap_call_with_release_slot`. `emit_decrefs_for` then
    // emits `LoadLocal slot + DecrefValueRegion(expected)` at
    // the Eval's decref_point; the runtime gate skips the decref when
    // `region_of(value)` doesn't match the placeholder — safe by
    // construction.
    //
    // Without this wiring (pre-fix), the walk's `alloc_here` for
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
#[ignore = "region merging not yet implemented"]
fn decref_region_emitted_once_for_merged_pair() {
    // `(let [x (string "a") y (string "b")] (g x y))` has two
    // allocations with identical decref_point and no cross-region
    // edges, so the merge pass collapses them into one region.
    // The lowerer emits exactly one `DecrefRegion` for the
    // merged group.
    let module = compile_to_lir("(let [x (string \"a\") y (string \"b\")] (g x y))");
    assert_eq!(
        count_decref_regions(&module),
        1,
        "merged x and y should share one DecrefRegion",
    );
}

// The native-tail ReturnValue retain (the `IncrefValueRegion` the post-
// `TailCall` block emits on the native-completion fall-through) is guarded by
// Elle corpus tests, not a Rust LIR-structural assertion: the non-splice path
// by region-native-tail-return-uaf.lisp (a true UAF witness, RED before the
// fix under guardfree) and the splice/`apply` path by
// region-splice-tail-return.lisp (a correctness guard — the splice UAF is
// masked by the args-array leak, so it asserts the result value instead).
