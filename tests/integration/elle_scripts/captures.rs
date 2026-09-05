// audited: 2026-09-05
// Guardfree pins for capture cells, letrec members and self-recursive closures.
//
// docs/analysis/testing.md

use super::*;

// Fusing a CAPTURING lambda (docs/impl/dissolution.md § "Captures") splices a body
// that reads an enclosing binding directly, so the loop holds a heap value the
// ENCLOSING frame owns rather than one reached through a closure environment. Three
// roles follow from that. The capture is read once per element across the whole
// walk, so the frame must still own it at the last one; the body may hand it INTO
// the accumulator, so a frame-owned value ends up in a structure that outlives the
// loop; and a captured mutable binding is written per element through its cell,
// whose displaced prior must free without taking the live one. Any of those
// over-frees SIGSEGVs under guardfree. Fires only on the fused shape; the plain-VM
// run asserts the values.
#[test]
fn region_capture_fuse_uaf() {
    run_elle_script_with_args(
        "region-capture-fuse-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a self-recursive closure that is ALSO captured by a sibling (so it is
// cell-held) must NOT have its region released by a tail-call deferred release: the
// capturing cell owns that release, and its lifetime outlives the tail-call
// activation. Marking such a binding `stranded_self` frees its region under the
// live cell (a generation panic / SIGSEGV under guardfree at the next
// `tail_callee_release_region` deref). This is the scheduler's mutually recursive
// `handle-fiber-after-resume` group, whose regression SIGSEGVs
// tests/elle/process-io.lisp. Only CELL-FREE self-recursion is stranded
// (docs/impl/selfrec.md).
#[test]
fn region_selfrec_captured_tail_release() {
    run_elle_script_with_args(
        "region-selfrec-captured-tail-release",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a stranded recursive closure the recursion RETURNS keeps its tail-call
// deferred release, and that release must drop only the FRAME's reference. The
// caller's is minted by the callee's `Return`, which runs before `trampoline_loop`
// breaks and fires the deferred decref (docs/impl/selfrec.md § the deferral's escape
// gate). If the count is wrong, the returned handle's region is recycled and the
// self-call re-dispatch — which reads the executing closure out of that very region —
// derefs a foreign page. Covers all three stranding routes (`letrec` self, `def` self,
// merged mutual SCC) with allocation churn between the release and the re-entry.
#[test]
fn region_selfrec_return_release() {
    run_elle_script_with_args(
        "region-selfrec-return-release",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a stranded recursive closure handed across the FIBER frontier keeps its
// tail-call deferred release too, and that release must drop only the FRAME's
// reference. The crossing counts its own: the emit's park retain into `fiber.signal`,
// which the resumer's result release consumes, and `chan/send`'s send-site incref,
// held until a receive builds the result carrying the message (docs/impl/selfrec.md §
// "The deferral needs no escape gate"). If the count is wrong, the delivered handle's
// region is recycled and the self-call re-dispatch — which reads the executing closure
// out of that very region — derefs a foreign page. Drives both fiber-frontier seeds —
// the emit through both binder routes, the send through the `letrec` one — with
// allocation churn between the release and the re-entry.
#[test]
fn region_selfrec_fiber_release() {
    run_elle_script_with_args(
        "region-selfrec-fiber-release",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a local mutual-recursion clique (`ev`/`od`) one of whose members is RETURNED
// must keep that handle live and re-enterable after its merged arena's release runs. The
// merge admits the return facet because the returned member lives IN the arena, so the
// callee's `Return` mint raises the arena's own count, and the member-callee tail
// deferral runs at the recursion's normal completion — after that mint — dropping only
// the frame's own reference. Were the ordering the other way (or the deferral the last
// reference), the caller would hold a closure whose env sits in a freed arena: a
// generation panic on the plain VM, a SIGSEGV under `--trace=guardfree`. Every returned
// handle is re-entered after the release across allocation churn that recycles a freed
// page, including handles held live across many later mint/free cycles, plus the refused
// residual (a non-member body tail) which must still run correctly.
// docs/impl/region/letrec.md § The frontier gate.
#[test]
fn region_letrec_return_cycle_uaf() {
    run_elle_script_with_args(
        "region-letrec-return-cycle-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// A top-level mutable that is BOTH captured by a closure (boxed in a
// MakeCaptureCell) AND reassigned. The hazard: routing the init value's release
// through the binding slot — which holds the CELL — makes `DecrefValueRegion`
// reload the slot and (via `result_region_of`, which unwraps a capture cell)
// free whatever the cell holds at the decref's runtime firing point. Once a
// reassignment has repointed the cell, that would free a different, live value
// (UAF) and double-release the displaced original.
//
// The lowerer avoids this by routing such a binding's init via
// `Lowerer::store_captured_cell_init`: it drops the init's alloc reference off
// the value register directly (timing-independent) and SKIPS the cell-slot
// routing (`region_info.captured_reassigned_bindings`). A captured binding that
// is never reassigned keeps the ordinary routing — its cell content is stable,
// so the unwrap always names the right value.
//
// Quarantined under tests/integration/fixtures/ (not tests/elle/) and pinned
// with `--trace=guardfree` so that a regression faults deterministically (rather
// than landing on a recycled page) in its own subprocess, instead of aborting
// the shared `make smoke` process.
#[test]
fn region_capture_cell_reassign_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-reassign-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// The same hazard with the `assign` moved INSIDE a closure the defining scope
// encloses. The binding still owns a compiled `MakeCaptureCell`, so the routing
// question is unchanged — but classifying the reassign by the ASSIGN SITE's scope
// calls it fn-local, keeps the cell-slot routing, and frees the value the frame
// hands back. The classification is a fact about the binding, not the write site
// (docs/impl/region/bindings.md § "Captured reassigned cells"). Compile-level
// twins over more shapes: `lir::lower::tests::release`'s
// `*_closure_reassign_leaves_no_cell_slot_release`.
#[test]
fn region_capture_cell_closure_reassign_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-closure-reassign-uaf.lisp",
        &["--trace=guardfree"],
    );
}

// GREEN (live guard) — the BENIGN, non-reassigned sibling of the hazard above: a
// top-level mutable captured by one or more closures (boxed in a MakeCaptureCell)
// but never reassigned. The RC accounting balances; the hazard here is
// COMPILE-ORDER nondeterminism, which the lowerer pins down two ways:
//
//  1. `compute_last_use`'s binding-chain override must resolve each chain
//     fully, not a hash-ordered PREFIX. acc's cell reaches the `(u1)` call
//     site only through u1's override (the capture-use registers at the Lambda
//     node, which IS u1's init id); resolving acc first would land its cell's
//     decref_point before the closure calls — freed while still callable. The
//     override is iterated to its unique, order-independent fixpoint in
//     src/hir/liveness/lastuse.rs.
//  2. At the shared decref_point, the cell's page-FREEING `DecrefRegion` must
//     order after the init's page-READING `DecrefValueRegion` (which unwraps
//     the cell); a freeing-first permutation would tear the page the unwrap
//     reads. The topological release order in `Lowerer::with_region_info`
//     (`order_releases`) tie-breaks page-reads before page-frees, fixing the
//     order (docs/impl/region/rules.md Rule 4).
//
// A regression would fault only timing-dependently (~⅓ of runs under
// guardfree), so the guard loops: 25 runs at p≈0.37 witness a regression with
// probability > 0.9999, while the correct release path passes every run.
// Compile-level twins of this guard live in src/lir/lower/tests.rs
// (`release_order_value_gated_before_plain_in_shared_bucket`,
// `region_analysis_is_deterministic_across_compiles`).
#[test]
fn region_capture_cell_noreassign_uaf() {
    for _ in 0..25 {
        run_elle_file_with_args(
            "tests/integration/fixtures/region-capture-cell-noreassign-uaf.lisp",
            &["--trace=guardfree"],
        );
    }
}

// Guard — a `@`-mutable captured local materialized as a `populate_env` env cell
// (minted once per activation) and captured by a closure built in a loop must
// survive every iteration. The closure captures the CELL by indirection — a BORROW
// through a separately-owned env cell whose release is hoisted to once-per-activation
// — so the ownership forest must NOT fold the cell's contents into the closure's
// per-iteration Owned subtree. If it did, the closure's subtree drop would free the
// cell (and its still-referenced contents) at the end of iteration 1, and the next
// iteration's re-store of the cell derefs the freed page (`capture_store_with_rebind`
// reads the stale prior content). `capture_containment_edges` excludes cell-indirected
// captures for exactly this reason (the cell owns its contents, the closure only reads
// through it). The corpus runner exercises the (now unconditional) forest but never
// under `--trace=guardfree`, so this subprocess is the deterministic-fault guard for
// the env-cell-vs-capture-adopt interaction. Canonical shape:
// tests/elle/region-capture-cell-loop-uaf.lisp (single loop, nested loops, and
// per-iteration content variance).
#[test]
fn region_capture_cell_loop_uaf_ownership() {
    run_elle_script_with_args("region-capture-cell-loop-uaf", &["--trace=guardfree"]);
}

// GREEN (live guard) — distinct from `region_traits_table_uaf` above, which was a
// RUNTIME RC gap (fixed). This is the COMPILE-TIME OWNERSHIP invariant the unconditional
// forest upholds: a closure captures a top-level struct and attaches it as a trait table
// with `with-traits`. `with-traits` declares `RegionEffect::Fresh` AND `embeds: &[1]`, so
// the walk records the `result ⊇ table` embed containment (`call_embeds` →
// `containment_edges`) — the compile-time analog of the runtime alloc-scan that counts the
// same embedding. With it the forest sees the captured table flow OUT through the escaping
// traited value and keeps it Shared instead of capture-adopting it. Without it the closure's
// subtree drop frees the table while the escaped value's `traits` field still references it
// — a wrong answer (`nil`) on plain runs and a SIGSEGV (context `UpdateCapture`) under
// guardfree. Quarantined as a subprocess because a regression is an uncatchable SIGSEGV that
// would crash the shared smoke harness; armed under guardfree so the fault is deterministic
// if it returns. Full repro + invariant in the fixture.
#[test]
fn region_traits_capture_adopt_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-traits-capture-adopt-uaf.lisp",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The STRING sibling of region_fold_closure_arg_uaf: a helper accumulates a
// string by reassigning a `@`-capture cell in a loop (`(assign out (string out
// …))`) and RETURNS `out`; the caller reads the returned value one form later.
// Green pins that the loop-reassigned capture cell's returned region stays live
// across the caller's read (the mu `_safe-uri` / `_slug` builder shape). Runs as
// a guardfree SUBPROCESS via `run_elle_file_with_args`, so an over-free's
// SIGSEGV fails THIS test cleanly instead of taking a shared harness process
// down (that hazard is the tests/elle/*.lisp glob's shared process, not this
// one). Full shape in the fixture header.
#[test]
fn region_capture_cell_string_accum_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-string-accum-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Two sequential loops over ONE reassigned mutable chain the versions
// functionalization gives the name (`last#2 <- last#1 <- last#0`), so a middle
// version carries a 1-slot cell whose content is the reference the chain
// forwards on (docs/impl/region/bindings.md § "A chain of forwarding edges hands
// one reference along, so the fold follows it whole"). Green pins that exactly
// one link releases that reference: the cell holding it when its own slot is
// overwritten, or the last link at its scope demise. Subprocess guardfree run,
// same rationale as the twin above; the leak and read-back faces are in
// tests/elle/region-cell-forward-chain.lisp. Full shape in the fixture header.
#[test]
fn region_cell_forward_chain_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-cell-forward-chain-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// A fn-local 1-slot container whose INIT value carries a second name takes that
// value by a COUNTED store rather than by donation, so the alias keeps the
// producer's reference and the decref that releases it
// (docs/impl/region/bindings.md § "What the cell donates it must hold alone;
// what it counts it need not"). Green pins that every later read through the
// alias — after the cell has displaced the init, or after a cursor has walked
// off the chain head the alias names — still reads a live page. Subprocess
// guardfree run, same rationale as the twin above; the leak and read-back faces
// are in tests/elle/region-cell-aliased-init.lisp. Full shape in the fixture
// header.
#[test]
fn region_cell_aliased_init_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-cell-aliased-init-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// The CASCADE / stored-member twin of region_capture_cell_string_accum_uaf, and
// the e2e witness of the drop-time external-reference rescue
// (docs/impl/region/ownership.md § "The incoming edge table and the external-
// reference rescue"). A server fiber reads a request off a socket, stores a
// MEMBER of the parsed request (`(get req :params)`) into a module-level
// `@`-capture cell, then reads a SIBLING member (`(get req :id)`) inside a
// `protect` sub-fiber to frame the reply — so `req`'s region is capture-adopted
// into that fiber's closure and would die with its subtree drop at the fiber's
// completion, while the cell still holds the `:params` member inline in it (the
// mu lib/cont/ipc.lisp driver-callback shape: `(assign got-X params)` while the
// same dispatch reads the request's id). Green pins that the drop rescues the
// externally-referenced region to the RC baseline: the cell's read after the
// fiber completes sees the live member, and the region frees at the cell's
// release. The rescue unit family is `regionstore::tests::forest`. Subprocess
// guardfree run, same rationale as the twin above.
#[test]
fn region_capture_cell_member_cascade_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-capture-cell-member-cascade-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — a fn-local 1-slot container's content needs BOTH of the cell's release
// channels (docs/impl/region/bindings.md § "Reassigned mutable bindings are 1-slot
// containers"): drop-on-overwrite for each displaced prior, the content drop at the
// cell's demise for the final one, with the producer's separate claim released at the
// store. Leak faces assert bounded region growth across a loop-carried cell, a cell
// bound inside the loop body, and a cell written once. The over-free faces read the
// content back inside the loop, after it, and out of a container that outlives the
// cell — so a demise that fires early, or a producer release that frees what the cell
// still holds, faults here rather than recycling silently. Full mechanism in the
// fixture header.
#[test]
fn region_fn_local_cell_drop_uaf() {
    run_elle_script_with_args(
        "region-fn-local-cell-drop-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — stdlib `compose`/`comp` compose correctly, no over-free. A
// self-tail-recursive HOF (stdlib `fold`'s letrec `go`) reached from >= 2 call
// sites in a unit must not over-free a value its tail call transferred forward.
// The region walk's callee inline (`try_inline_call`, whose sole job is to
// surface a callee body's cross-region EDGES at the call site) binds the
// callee's params to the CALLER's arg regions; a `Return` reached inside that
// re-walk names the arg region, not the value the callee structurally returns.
// Recording it in `return_sites` pins the transferred arg's `decref_point` to
// the callee's base-case (sibling) arm, and under self-tail-call frame reuse the
// branch-union release over-frees the reducer result the tail call already moved
// into the next accumulator. The interprocedural return facet is escape.rs's
// authority (a summary, not a re-walk), so the inline records return-frontier
// extensions only on the structural walk — mirroring the `inline_depth == 0`
// gate the Letrec/Let cell mint already uses. `compose`/`comp` fold `identity`
// with a closure-returning reducer, so the composed closure is exactly such a
// transferred accumulator; this exercises the full user-visible surface plus the
// isolated single-step fold. Armed under guardfree so any regression faults
// deterministically at the freeing decref. The corpus witness is
// tests/elle/functional.lisp's compose section, surfaced by the batched smoke
// gate. Full mechanism in the fixture header.
#[test]
fn region_compose_closure_acc_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-compose-closure-acc-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — an os/spawn worker pre-allocates a capture cell for each of the
// spawned closure's captured LOCALS so the body's UpdateCapture/DecrefCellRegion
// find them in the env. Each such cell must live in its OWN region, never the
// worker's `recv_region`: the body owns the cells and frees them with
// `DecrefCellRegion` (value-resolved to the cell's region) at scope exit, so a
// cell in recv_region would drive recv_region's RC to 0 mid-body and the worker's
// cleanup `decref_region(recv_region)` then double-frees a phantom region. This
// subprocess runs the JIT tier under the guardfree oracle, where a regression
// faults deterministically on the worker thread. (`src/primitives/
// concurrency.rs`, the captured-local cell loop.)
#[test]
fn region_spawn_capture_mutate_guardfree() {
    run_elle_script_with_args(
        "region-spawn-capture-mutate",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
