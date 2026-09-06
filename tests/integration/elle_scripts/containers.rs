// audited: 2026-09-05
// Guardfree pins for container reads, native effects and the store funnel.
//
// docs/analysis/testing.md

use super::*;

// region-array-element-uaf is guardfree-clean (the call-index element survives
// the consuming native's borrow); armed under the UAF oracle to lock the retain,
// mirroring region_jit_passthrough. (The harness already covers its plain and
// interpreter-tier runs via the vm/jit policies.)
#[test]
fn region_array_element_uaf_guardfree() {
    run_elle_script_with_args(
        "region-array-element-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a cons cell (`%pair`) storing a HEAP value keeps that value's region
// alive for the cons's lifetime via the runtime alloc-scan/free-cascade contract
// (`handle_list` → `incref_cross_region`; `find_object_cross_refs` Pair arm), NOT
// a compile-time containment edge (which double-counts against the single cascade
// decref — the same RC double-count captures avoid by recording no edge). This
// reads a deep chain of escaping conses' heap contents back after region-id churn:
// an under-incref of any element frees it under the reader (SIGSEGV under
// guardfree). Pairs with the oracle's `arg-result`/`cons-store` leak pins (the
// over-keep face of the same edge). docs/impl/region/ownership.md § "The outgoing
// edge table"; walk/intrinsic.rs (the %pair contents).
#[test]
fn region_pair_heap_content_uaf() {
    run_elle_script_with_args(
        "region-pair-heap-content-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the sequence reads and conversions declare `Opaque`, which says two
// things: the result may live anywhere, and no argument is stored uncounted
// (docs/impl/region/effects.md § `Opaque`). The second withdraws a store-facet
// escape seed, and with it the refusal that seed forced on every mechanism gated
// on `frame_held_regions`. So this drives what the refusal used to mask: a
// read's result consumed after the branch that produced it, the subject read
// again, the result returned to a caller or yielded to a resumer, and a genuine
// store escape that must still refuse the window. Freeing the container under any
// of those faults — SIGSEGV under guardfree. The leak face is
// `region-sequence-read-effect.lisp`.
#[test]
fn region_sequence_read_effect_uaf() {
    run_elle_script_with_args(
        "region-sequence-read-effect-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a resume value delivered into a frame parked at a suspending PRIMITIVE
// call carries one owning reference (docs/impl/region/owner.md § "A delivery into
// a replayed frame carries one owning reference"). The replayed frame re-enters at
// that call's continuation and runs the call's compiler-emitted result release; a
// bytecode callee funds that release with its `Return` mint, but a primitive that
// suspends never returns, so the delivery owes it. This drives both parks that
// land in such a continuation — a dynamic `emit`, in bound and tail position and
// held across a further park, and a mediated capability denial — each paired with
// the literal `Emit`, whose resume block mints the reference in bytecode and is
// therefore correct without the delivery's. Freeing the delivered value early
// faults on the read below — SIGSEGV under guardfree. The leak face is the
// `primitive-resume-*` closed-control family in `tests/elle/oracle.lisp`, which
// refuses a delivery that mints more than one.
#[test]
fn region_primitive_resume_uaf() {
    run_elle_script_with_args(
        "region-primitive-resume-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN (live guard) — `with-traits` attaches a trait-table struct to a value;
// the table lives in its OWN region (not inline), so it is a cross-region edge
// like any content field. `find_object_cross_refs` must enumerate the `traits`
// side-field, not just the inline content fields: otherwise the table keeps
// RC 1 and its constructor's DecrefValueRegion DIRECT-frees it while the host
// still references it — `(get (traits x) :tag)` then binary-searches the freed
// struct → SIGSEGV in TableKey::cmp (types.rs). `find_object_cross_refs`
// enumerates `obj.traits()` for all variants, so the alloc-scan increfs and the
// free-cascade decrefs symmetrically (Rule 5/7). Quarantined as a subprocess
// because a regression is an uncatchable SIGSEGV that would crash the shared
// smoke harness; armed under guardfree so the fault is deterministic if it
// returns.
#[test]
fn region_traits_table_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-traits-table-uaf.lisp",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN guard — the stdlib `pop` wrapper (`%pop`/`%pop-string`/`%pop-bytes`) stays
// balanced across all three mutable container types. The @array arm suppresses its
// moved-out element's redundant tail retain (and extracts an Owned element); the
// @string/@bytes arms return a FRESH grapheme / immediate that must KEEP their tail
// retain — over-suppressing them over-frees the returned grapheme (the Q1 hazard).
// The wrapper's owned-param container also strands across the match arms and is freed
// per-arm by the container compensation. Runs clean; a regression that unbalances any
// arm SIGSEGVs here. Full detail in the fixture header.
#[test]
fn region_pop_wrapper_types() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-pop-wrapper-types.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the general container-READ-escape sibling of the pop moves-out UAF. A heap
// element pushed into a LOCAL Owned @array via a raw `%array-push` is adopted into the
// container's Owned subtree, so reading it back out with `first`/`get`/`rest` and
// letting the result ESCAPE must NOT leave it interior — else the container's
// scope-exit subtree drop frees it under the escaped reference (a stale-region deref
// once ids recycle). Escape propagates through the container read (`analyze_escape`'s
// read-result → container-contents edge): an escaping element-read marks the
// container's stored contents escaping, so the ownership forest refuses to adopt them
// and the ordinary RC path keeps them live across the caller's read. DISTINCT from the
// pop case (a read BORROWS — the element stays in the container — so the fix is escape
// marking, not pop's extract). Runs clean; a regression that re-admits the adopt
// SIGSEGVs here. Full repro + trace in the fixture header.
#[test]
fn region_container_read_escape_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-container-read-escape-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the container-read BORROW, the LOCAL sibling of the escape face above: a
// value read out of a container with `get`/`first`/`rest` still lives INSIDE that
// container, and the two read forms are kept alive differently. An OPCODE read
// (`%get`/`%first`/`%rest`) raises no count, so the container's lifetime is the
// borrow's only protection and its `decref_point` extends to the reader
// (docs/impl/region/rules.md Rule 4, the borrowing node) — this bites even with a
// PARAM container, no ownership subtree in play. A NATIVE read takes the Rule 5
// pass-through retain, which the RC baseline honours but ADOPTION freezes: the
// ownership cut must refuse a subtree whose member a read alias can still name, and
// order the alias's page-reading release ahead of the container's where the two
// coincide. The fixture drives every face past a priming loop and then asserts
// region-count bounded, so a regression in either direction — over-free, or a
// stranded lifetime — is loud. Full mechanism in the fixture header.
#[test]
fn region_container_read_borrow_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-container-read-borrow-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the counted container read is retained by every BINDER FORM that records
// it (docs/impl/region/bindings.md § "Every binder form that records the read must
// emit the retain"). A name bound to a whole-value read of a re-storing container
// borrows a reference the next overwrite releases, so the reader takes one of its
// own — and the container is handed its donation on the strength of that. The
// analysis records the read from both binder arms of the walk, so a module-scope
// reader (the file-letrec binder) is as exposed as the fn-local `let` reader
// tests/elle/region-reassign-captured-cell-reader.lisp pins. Emitting the retain in
// only one of them runs both halves of the bargain against a reference nobody took:
// the overwrite frees the value under the reader, and the reader's own placeholder
// release decrefs it again. Full shape in the fixture header.
#[test]
fn region_container_read_toplevel_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-container-read-toplevel-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the opaque CALL between a container and the value read out of it. A funnel
// adopt freezes its member's RC, so the forest owes a bound on every alias that can name
// the member; it reads those bounds off a native read whose container argument IS the
// container. A call hides that: `(concat a @[1 2])` on a MUTABLE first argument returns
// `a` itself, so reading out of the call's result is reading out of `a` under a
// placeholder that relates to no member; and `(last a)` hands back the adopted element
// directly. Only `Fresh`/`Stores`/`Sends` (a result in the call's own minted region — the
// claim the effects oracle checks) or `Immediate` rule the aliasing out. The fixture
// drives both faces past a priming loop, then samples each shape's per-op region growth:
// the returned member must read bounded (the refusal puts it back on an RC baseline that
// still reclaims it), and the concat shapes are pinned shrink-only over the per-call
// residue a mutable-first-argument `concat` carries on its own. So a regression in either
// direction — over-free, or a subtree traded for a leak — is loud. Full mechanism in the
// fixture header.
#[test]
fn region_call_result_alias_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-call-result-alias-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — a mutable @set del of a HEAP member must release the STORED member's
// region, not the caller's lookup value. `(del s x)` removes the element
// value-EQUAL to `x`; for a heap member the stored element and `x` are two
// distinct allocations in distinct regions, and the add half recorded the
// outgoing edge / incref against the stored member's region. A set remove that
// resolves the un-record + decref from `x` (a bare `BTreeSet::remove` yields no
// element) un-records an edge that was never recorded — outgoing-edge accounting
// drift the debug equivalence oracle detonates on — and over-frees the caller's
// live region under guardfree, while the stored member leaks. `set_del_with_decref`
// resolves both from the member `take` hands back, mirroring the @struct/@array
// removes. Quarantined as a subprocess because a regression ABORTS (oracle panic /
// guardfree fault) and would take the shared smoke harness down. Full repro +
// invariant in the fixture.
#[test]
fn region_set_del_heap_member_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-set-del-heap-member-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// Guard — the F1b container compensation must release ONLY the store wrapper's
// stranded owned-param reference, never a live container. A polymorphic
// `push`/`put`/`add` reached as a value runs its `(match (type-of coll) …)` body,
// whose mutable arm tail-calls a `-mut` funnel returning the container arg0
// pass-through; the wrapper leaks its owned-param reference to that return-escaping
// container (1/op). The close balances it with a per-arm release in the wrapper
// body (`regions::compensate`, `funnel_container_sites`) plus suppressing the
// redundant tail ReturnValue retain (`lir::lower::control::call`). Because the
// funnel's `pass_through_retain` already handed the caller one owning reference,
// releasing the owned-param reference can never drop the live container to zero —
// but an over-aggressive release would free a container the caller still holds. The
// fixture builds ESCAPING array/set accumulators and a nested pass-through wrapper,
// reading every stored element back across an id-recycling loop, so such an
// over-free faults under guardfree. Quarantined as a subprocess because a
// regression ABORTS (guardfree fault / oracle panic) and would take the shared
// smoke harness down. Full repro + invariant in the fixture.
#[test]
fn region_mut_container_compensation_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-mut-container-compensation-uaf.lisp",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a struct storing a HEAP-valued key (a list/bytes/set/struct used as a
// key, held as `TableKey::Heap(Value)`) records the outgoing content edge
// `region(struct) → region(key)` and increfs the key's region, exactly as it
// does for a struct VALUE. The key value is built in the caller's region and
// pointed at from the struct's region — a cross-region reference the alloc-time
// scan (`find_object_cross_refs`) must enumerate so the free-time cascade
// balances it. Enumerating only the values (the old struct arms) left the key's
// region reclaimed at its constructor's decref_point while the struct still
// pointed into it: a stale key comparison on the next `get`/`put` (binary search)
// derefs the freed page, and — because the drifted region gets reused — reads
// live-but-wrong data, silently collapsing distinct compound keys onto one slot.
// Quarantined as a subprocess because a regression ABORTS (guardfree fault /
// oracle panic) and would take the shared smoke harness down. Full repro +
// invariant in the fixture.
#[test]
fn region_struct_heap_key_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-struct-heap-key-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// Guard — the MUTABLE-@struct twin of the above: an in-place `put` that ADDS a
// heap-valued key records the `region(struct) → region(key)` edge and increfs the
// key, and a `del` un-records + decrefs it (`struct_put_with_rebind` /
// `struct_remove_with_decref`). The alloc-scan handles keys present at
// construction, but an in-place put adds a key AFTER allocation, so the store
// funnel must record it — enumerating only the value left the free-time content
// scan (which walks keys) disagreeing with the recorded edge table, a missed
// store-funnel edge the equivalence oracle detonates on. Quarantined as a
// subprocess because a regression ABORTS. Full repro + invariant in the fixture.
#[test]
fn region_struct_mut_put_heap_key_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-struct-mut-put-heap-key-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// Guard — a leaf helper called many times from a driver. The callee closure lives
// in a letrec forward-reference cell the driver captures BY INDIRECTION (an
// uncounted cell store the ownership scan cannot see). The forest must treat that
// capture as a borrow (`needs_capture` in `capture_containment_edges`), never
// folding the callee's region into the driver's Owned subtree nor claiming it Owned:
// the closure region reclaims on the per-region-RC baseline (kept live by the
// runtime auto-incref over the driver's env). Adopting it — the acyclic
// forward-reference the closure-cycle MERGE does not cover — defers the cell's free
// past the closure's own, so a later region's free-time cross-ref scan reads the
// reclaimed closure page. Runs under guardfree so a regression faults
// deterministically at that stale read; the harness runs the file under its vm/jit
// policies WITHOUT the oracle, where the read is stale-but-intact and the
// bounded-growth asserts pass. Canonical shape:
// tests/elle/region-repeated-call-adopt-uaf.lisp.
#[test]
fn region_repeated_call_adopt_uaf() {
    run_elle_script_with_args(
        "region-repeated-call-adopt-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
