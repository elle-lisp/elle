// Elle scripts that must run under a PROCESS-GLOBAL runtime mode the `elle test`
// harness cannot vary per file.
//
// The corpus under tests/elle/ is owned by the agent-first runner (`elle test`,
// via `make smoke`/`smoke-elle`): it compiles and runs EVERY tests/elle/*.lisp
// once per JIT policy (`:off`→`vm`, `:eager`→`jit`) plus per-tier divergence for
// single-form files — strictly more than a one-off `elle FILE` run. So a plain
// "run this .lisp and assert exit 0" test here is pure duplication; those have
// been removed (see docs/testing.md, docs/test-runner.md).
//
// What the harness CANNOT do is set a process-global mode for one file: the
// page-guard UAF oracle (`--trace=guardfree`), the I/O backend (`--no-uring`),
// or a backend toggle paired with the adaptive JIT (`--jit=adaptive --mlir=off`).
// These live in config.rs as static, once-per-process settings (the runner
// shares one process across every file's worker thread), and a guardfree UAF
// deliberately SIGSEGVs — which would take the single-process harness down with
// it. So the few files that must run under such a mode are pinned below, each as
// its own subprocess `elle <flags> FILE`. (The eventual home is per-file mode
// declarations the runner honors — docs/test-runner.md § future work.)

use std::process::Command;

fn get_elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run tests/elle/{name}.lisp with `extra_args` (the process-global backend/trace
/// flags that motivate keeping the script here) and assert it exits with code 0.
///
/// Panics with stdout+stderr if the script exits non-zero or fails to spawn.
fn run_elle_script_with_args(name: &str, extra_args: &[&str]) {
    run_elle_file_with_args(&format!("tests/elle/{}.lisp", name), extra_args);
}

/// Like `run_elle_script_with_args` but takes a path relative to the crate root,
/// for reproducers QUARANTINED outside tests/elle/ (e.g. a script that aborts on
/// plain runs, which would take the shared `make smoke` harness process down).
fn run_elle_file_with_args(script: &str, extra_args: &[&str]) {
    let elle_bin = get_elle_binary();

    let mut cmd = Command::new(elle_bin);
    cmd.args(extra_args).arg(script);
    let output = cmd
        .output()
        .unwrap_or_else(|e| panic!("Failed to spawn elle for {} {:?}: {}", script, extra_args, e));

    assert!(
        output.status.success(),
        "Elle script {} {:?} failed (exit {:?}):\nstdout: {}\nstderr: {}",
        script,
        extra_args,
        output.status.code(),
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr),
    );
}

// =============================================================================
// The guardfree UAF oracle (`--trace=guardfree --jit=adaptive --mlir=off`)
// =============================================================================
//
// `--trace=guardfree` mprotects every freed page PROT_NONE and leaks the
// mapping, so a use-after-free faults (SIGSEGV) at the exact dereference instead
// of silently reading a recycled slot. The harness runs these files under its
// vm/jit policies WITHOUT the oracle, where a regression UAF would read recycled
// memory and show a false green — so the deterministic-fault coverage only exists
// here. `--jit=adaptive` (not the harness's `:eager`) is load-bearing: several of
// these defects only manifest when the adaptive tier JIT-compiles the hot builder
// while pass-through results are still live.

// Region regression: a JIT-compiled function calling a native "pass-through"
// primitive (`first`/`rest`/`get`) must apply the same pass-through retain as
// the interpreter, or the result region is under-counted and freed while a
// freshly built cons still references it (UAF).
#[test]
fn region_jit_passthrough() {
    run_elle_script_with_args(
        "region-jit-passthrough",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

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

// Guard — a top-level mutable reassigned to a value referencing its old content
// (`(assign x (pair v x))`) must survive when the file runs as the `%file-body`
// whole-module thunk (the `elle test` shape). The solver must classify the
// file-letrec binding as module-scope (not fn-local via a spurious `in_lambda`),
// so the dead `__file_expr_N` statement wrapper's slot-routed decref does not
// free the just-stored value under the cell: `is_file_scope` routes it to the
// top-level container model. The advanced.lisp `match in loop` shape.
#[test]
fn region_toplevel_reassign_thunk_uaf() {
    run_elle_script_with_args(
        "region-toplevel-reassign-thunk-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `match` pattern binding that aliases into the scrutinee's region
// (`(a & rest)`, `(h . t)`, an immutable-array element, an immutable-struct
// value) must record the scrutinee's `binding_regions`, so the subject region's
// decref_point extends over the bound alias and the subject is not freed under
// the consumer's borrow (which would SIGSEGV under guardfree). The solver's
// `Match` arm propagates the scrutinee's regions to each arm binding, mirroring
// the `Destructure` HIR node. The advanced.lisp guard-with-rest shape.
#[test]
fn region_match_rest_uaf() {
    run_elle_script_with_args(
        "region-match-rest-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — native-tail-return of a heap pass-through result (`(first xs)`/
// `(get xs 0)`/`(xs i)`) must keep the ReturnValue retain, so the caller's
// DecrefValueRegion does not free it under its borrow (which would SIGSEGV
// under guardfree). The native-tail post-block retains a heap result before
// Return.
#[test]
fn region_native_tail_return_uaf() {
    run_elle_script_with_args(
        "region-native-tail-return-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a whole-value read of a REASSIGNED CAPTURED CELL (fn-local upvalue
// read AND module-scope `def @cell`) must take a counted reference, or the
// cell's next overwrite (`capture_store_with_rebind` decrefs the displaced prior
// unconditionally) frees the value under the reader — the captured-alias UAF
// (SIGSEGV under guardfree). The reader takes Rule 5's "new reference"
// pass-through (an `IncrefValueRegion` at the read, balanced by the
// `DecrefValueRegion` at its last use). This is the std/process scheduler's
// `ready` double-buffer (`sched-run`'s `(let [batch ready] (assign ready @[])
// (each pid in batch (run-one pid)))`), whose regression SIGSEGVs
// tests/elle/process-io.lisp. docs/impl/region/bindings.md § "Captured
// reassigned cells".
#[test]
fn region_reassign_captured_cell_reader() {
    run_elle_script_with_args(
        "region-reassign-captured-cell-reader",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a self-recursive closure that is ALSO captured by a sibling (so it is
// cell-held) must NOT have its region released by a tail-call adopt: the
// capturing cell owns that release, and its lifetime outlives the tail-call
// activation. Marking such a binding `stranded_self` frees its region under the
// live cell (a generation panic / SIGSEGV under guardfree at the next
// `tail_callee_adopt_region` deref). This is the scheduler's mutually recursive
// `handle-fiber-after-resume` group, whose regression SIGSEGVs
// tests/elle/process-io.lisp. Only CELL-FREE self-recursion is stranded
// (docs/impl/selfrec.md).
#[test]
fn region_selfrec_captured_tail_adopt() {
    run_elle_script_with_args(
        "region-selfrec-captured-tail-adopt",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a local mutual-recursion clique (`ev`/`od`) whose `letrec` body ends in a
// tail call to a NON-member (a native `%add`, the redefined-closure operator `+`, a
// foreign fn `g`, and a MIXED member+non-member `if`) must reclaim its merged arena
// soundly. The frame-replacing `TailCall` strands the arena's binding-scope drop, so
// a closure callee rides the explicit arena adopt (`TailCall::adopt_region_slot`) at
// recursion completion while a native callee falls through to the live scope-exit
// drop — mutually exclusive per call, exactly one release. A premature free leaves
// `ev`/`od` (whose regions ARE the merged arena) dereferencing recycled pages on the
// next recursion step (SIGSEGV under guardfree). Also drives the clique PER LOOP
// ITERATION, the per-call reclamation granularity an activation-owner-node cut would
// double-free. docs/impl/region/letrec.md § The letrec closure-cycle merge.
#[test]
fn region_native_tail_mutual_cycle_uaf() {
    run_elle_script_with_args(
        "region-native-tail-mutual-cycle-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a fresh `%pair` pushed into a fresh, let-bound `@[]` whose push result is
// DISCARDED, in a loop, must reclaim its Owned subtree without a double-free. The pair
// is a store-adopted member whose own slot-resolved `DecrefRegion` is a no-op only
// while it is still `Owned`, so it must be emitted before the container's subtree drop.
// At the let-body the pair and the container share a `decref_point`, and the container
// is freed by TWO releases there (its binding release and the discarded pass-through
// result of `%array-push`, which returns its container); order the pair's plain
// `DecrefRegion` after those and the drop reclaims the pair before its own decref — a
// phantom/double-free (SIGSEGV under guardfree). The topological release order over the
// adopt edge (`with_region_info::order_releases`, member → owner) keeps the member's
// release ahead of the container's.
// docs/impl/region/adopt.md § "The lifetime obligation the root carries".
#[test]
fn region_array_push_pair_loop_uaf() {
    run_elle_script_with_args(
        "region-array-push-pair-loop-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Correctness guard — the splice/`apply` manifestation of the native-tail-return
// retain. `(first ;argv)` lowers to `TailCallArrayMut`, whose post-block emits
// the ReturnValue retain (lower_call splice arm). Known limitation: the splice
// UAF is currently masked by the args-array leak, so this asserts the result
// value rather than faulting; it becomes a hard UAF guard once that leak is
// fixed. Run under guardfree to lock the retain alongside the non-splice guard
// above.
#[test]
fn region_splice_tail_return() {
    run_elle_script_with_args(
        "region-splice-tail-return",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN (live guard) — a closure that tail-passes a TOP-LEVEL binding as an
// owned-param argument must not over-free that binding's region
// (phantom/double-free; SIGSEGV under guardfree). The hazard the witness
// describes does NOT reproduce on HEAD: `tail_arg_is_borrowed`
// (src/lir/lower/control.rs) still flags ONLY captured upvalues, so a top-level
// reference is indeed pure-moved into the owned-param callee — yet the 500-iter
// loop is guardfree-clean. The over-free the witness hypothesized is balanced
// elsewhere: the top-level escape is increfed through the Rule 5 EscapeSite
// funnel, so the callee's owned-param release leaves the region's RC intact.
// Kept as a guard: if a regression drops that escape incref, the binding's
// region drains to zero mid-loop and the final read faults here. Pinned as a
// subprocess because the guardfree witness would be an uncatchable SIGSEGV.
#[test]
fn region_tail_move_toplevel_uaf() {
    run_elle_script_with_args(
        "region-tail-move-toplevel-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// GREEN (regression guard) — the HOF manifestation of the native-tail-return
// retain: a pipeline whose tail is `(map …)`/`(filter …)` (the
// dns/parse-resolv-conf shape). The post-`TailCall` `Return` retains the
// heap result (the native-tail ReturnValue retain, also covering splice-tail)
// so the returned collection survives the caller's release. Passes under
// guardfree; locks the retain against regressions.
#[test]
fn region_hof_tail_return_uaf() {
    run_elle_script_with_args(
        "region-hof-tail-return-uaf",
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

// RED (known over-free, not yet fixed) — the closure-arg OVER-FREE, the over-free
// twin of the F5 arg-retain gap. A fold-shaped helper that holds its combiner `f`
// by THREADING it as a recursive argument (or by a `def @`-cell accumulator)
// instead of CAPTURING it in a letrec lets `f`'s region reach refcount 0
// mid-drive: its `DecrefValueRegion` frees the closure and a later `UpdateCapture`
// derefs the freed page (SIGSEGV under guardfree, traced to `DecrefValueRegion of
// closure … context: UpdateCapture`). This is exactly what blocks src/core.lisp
// `fold`/`reduce` from dissolving their per-call `go` closure to zero — they use
// the guardfree-safe letrec-CAPTURE form instead, and the SAME drive over that
// form is clean, so the fixture isolates the threaded-arg / cell-held closure
// lifetime, not folding in general. It is state-dependent (faults only once region
// ids recycle onto the freed one), so the fixture discards results and drives
// ~8000 reps to reach the collision deterministically. #[ignore]'d because it
// SIGSEGVs today — an uncatchable fault that would take the shared `make smoke`
// harness down; when the region solver keeps a threaded/cell-held closure's region
// live across its whole use, this exits 0 under guardfree — un-ignore it then.
// Full repro + trace in the fixture header; assessment.md Stage 1 § soundness note.
#[test]
#[ignore = "RED: closure threaded as recursive-arg / held in a def@ cell is over-freed (DecrefValueRegion of closure -> UpdateCapture); un-ignore when fixed"]
fn region_fold_closure_arg_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-fold-closure-arg-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
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

// Guard — a `squelch`/`attune` wrapper closure run as a fiber body. The wrapper
// shares the inner closure's template and env (their backing lives in the INNER
// closure's region), but the wrapper VALUE itself lives in a fresh region. A fiber
// keeps its body's regions alive by scanning the body's `closure` for cross-region
// edges — env backing and template — AND its `closure_value` (the wrapper value it
// installs as the body's executing-closure register on resume). Omitting
// `closure_value` from that scan leaves the wrapper value's region uncounted: for a
// plain closure it coincides with the template/env region (still kept alive), but a
// squelch/attune wrapper puts the value in a DIFFERENT region, which then frees at
// its binding's decref_point while the fiber still holds it — the next region's
// free-time scan reads the freed page. Runs under guardfree so a regression faults
// deterministically at that stale read rather than reading a recycled page. The
// harness runs the file under its vm/jit policies WITHOUT the oracle, where the read
// is stale-but-intact and silent. Canonical shape:
// tests/elle/region-squelch-fiber-uaf.lisp (squelch + fiber, attune + yield).
#[test]
fn region_squelch_fiber_uaf() {
    run_elle_script_with_args("region-squelch-fiber-uaf", &["--trace=guardfree"]);
}

// Guard — a `@`-mutable parameter reassigned in its body, whose (post-reassign)
// value is MOVED into a tail call. The param is materialized as a capture cell the
// callee owns; the tail-call move must retain the moved value (the borrowed-tail-arg
// retain) BEFORE the param cell's last-use `DecrefCellRegion`, whose cascade frees
// the cell's contents — the very value being moved. Emitting the cell release first
// frees the moved value and the retain then reads a freed page (the owned-params
// double-release). `lower_call` defers the tail arg's decrefs so the retain orders
// ahead of them. Runs under guardfree so a regression faults deterministically at
// that stale read; the harness runs the file under its vm/jit policies WITHOUT the
// oracle, where the freed page is stale-but-intact and the functional asserts pass.
// Canonical shape: tests/elle/region-mutable-reassign-param.lisp (overwrite-return,
// multi-reassign chain, aliased-arg clobber, id-recycling loop).
#[test]
fn region_mutable_reassign_param_uaf() {
    run_elle_script_with_args("region-mutable-reassign-param", &["--trace=guardfree"]);
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

// Self-recursion correctness across control-flow boundaries, armed under the UAF
// oracle. A self-recursive local function must keep recursing as itself — same
// body, same captured environment — across a yield/resume, a tail-call frame
// replacement, or a value handoff. The corpus files assert the *values* (a stale
// self-reference returns a wrong-but-well-typed result the harness's vm/jit
// policies catch); these subprocess runs add the complementary guarantee that
// carrying the executing closure across each boundary reads no freed page — a
// botched self-identity that freed the live closure/env would fault here under
// guardfree rather than read recycled memory. `--jit=adaptive` exercises the
// hot-compiled path while the recursion is still in flight.
#[test]
fn recur_after_yield_guardfree() {
    run_elle_script_with_args(
        "recur-after-yield",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_after_tail_call_guardfree() {
    run_elle_script_with_args(
        "recur-after-tail-call",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_as_value_guardfree() {
    run_elle_script_with_args(
        "recur-as-value",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

#[test]
fn recur_entry_guardfree() {
    run_elle_script_with_args(
        "recur-entry",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// In-lambda MUTUAL recursion under the UAF oracle: the closure-cycle merge puts
// the ev/od pair and their forward cells in ONE arena, released either by the
// letrec binding-scope drop (non-tail body) or by the tail-call adopt at the
// recursion's normal completion (tail body). A mis-accounted release — the arena
// freed while a rotation is still in flight, or freed twice across the two
// channels — reads a freed page here and faults deterministically.
#[test]
fn recur_mutual_guardfree() {
    run_elle_script_with_args(
        "recur-mutual",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The adaptive-JIT build of the same entry-boundary coverage: the adaptive
// tier compiles a hot caller while its self-recursive callee is still
// interpreted — the compile-window shape the stdlib-HOF probe in the file
// exercises. The harness runs the file on the default (VM) tier; this
// subprocess covers the JIT half.
#[test]
fn recur_entry_jit() {
    run_elle_script_with_args("recur-entry", &["--jit=adaptive", "--mlir=off"]);
}

// =============================================================================
// Backend toggles paired with the JIT
// =============================================================================

// Guard — a JIT-compiled fiber that suspends mid-I/O must not over-release the
// yielded io-request region. `--mlir=off` pins the pure-JIT path (the invariant
// must not depend on the MLIR backend being present); the harness's vm/jit
// policies don't isolate this combination on an MLIR-enabled build.
#[test]
fn region_jit_io_suspend_uaf() {
    run_elle_script_with_args("region-jit-io-suspend-uaf", &["--mlir=off"]);
}

// =============================================================================
// I/O backend selection (`--no-uring`)
// =============================================================================

// Same script the harness already runs (`posix.lisp`), but forced onto the
// threadpool I/O backend on Linux via `--no-uring` — a process-global choice the
// harness cannot make per file. The threadpool path uses the same
// `SignalReceiver` / `kq_sig_read_blocking` / `sigfd_read_blocking` machinery as
// macOS, so this gates the threadpool signal flow (the signalfd EAGAIN-poll path
// on Linux and the EVFILT_SIGNAL worker-unblock + no-op sigaction path on
// macOS). Without it we'd only exercise the io_uring path on the Linux runner.
#[test]
fn posix_threadpool() {
    run_elle_script_with_args("posix", &["--no-uring"]);
}

// (Hygiene for syntax-case bindings is carried structurally — synthetic-ness
// lives on PatternBinding (src/syntax/expand/syntaxcase.rs) rather than being
// inferred from a name's string prefix. The regression for it lives in
// tests/elle/macros.lisp as a plain-run test, where the harness owns it.)

// Deep fiber/resume nesting must not consume the host call stack. The
// bytecode-VM path routes nested resumes through the SIG_SWITCH trampoline
// in `do_fiber_resume` (src/vm/fiber.rs), so 20000-deep nesting completes;
// pinned under the process-global `--jit=off` so the VM path is what runs.
// See the fixture header.
#[test]
fn fiber_deep_nesting_vm() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=off"],
    );
}

// Known limitation — the JIT resume path (`handle_fiber_resume_signal_jit`,
// src/vm/fiber/jit.rs) calls `do_fiber_resume` synchronously from JIT'd frames,
// which cannot be unwound by SIG_SWITCH, so JIT'd deep fiber nesting overflows
// the host stack and aborts the process (hence the quarantined fixture +
// #[ignore]). Lifting this requires the JIT resume to convert to the trampoline
// via its side-exit (YIELD_SENTINEL) machinery.
#[test]
#[ignore = "RED: JIT'd nested fiber/resume still recurses on the Rust stack and overflows at depth"]
fn fiber_deep_nesting_jit() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=eager"],
    );
}
