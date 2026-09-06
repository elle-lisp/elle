// audited: 2026-09-05
// Guardfree pins for where a release lands: the branch-arm window, the break window, and the binder pins.
//
// docs/analysis/testing.md

use super::*;

// A fn-local mutable accumulated across a `while` and handed back takes the
// 1-slot-container model, so each value the loop displaces is released at the
// overwrite and the last one leaves with the caller — the `Return`'s mint pays
// for the caller's reference and the cell's content drop, emitted after that
// mint, releases the cell's (docs/impl/region/bindings.md § "Returned fn-local
// reassigned mutables — the return claims the MINT's reference, not the
// cell's"). Armed under the UAF oracle because the model runs a free path the
// unsuppressed baseline never ran: were the content drop to consume the caller's
// reference instead, the returned chain would fault at the caller's read. The
// harness already covers the file's plain vm/jit runs and its bounded-rate face.
#[test]
fn region_loop_acc_return_uaf() {
    run_elle_script_with_args(
        "region-loop-acc-return",
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

// Guard — a match-destructured `rest` alias (`(a & rest)`) is a BORROWED subview
// of the scrutinee, but the region solver never registered a counted container
// read for the pattern load (only for call-site `rest()`/`first()`). Passing the
// alias as an owned-param call argument (tail or not) made the callee's param
// release free the caller's still-live scrutinee region — a use-after-free on
// the caller's original list (SIGSEGV/SIGBUS under guardfree). The lowerer now
// marks destructure-rest bindings borrowed so the call site mints a fresh
// owning reference the callee's release balances.
#[test]
fn region_match_rest_tail_move_uaf() {
    run_elle_script_with_args(
        "region-match-rest-tail-move-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — `break` TRANSFERS its value to the enclosing block
// (docs/impl/region/mechanism.md § "`break` transfers its value; it does not
// consume it"). The transfer moves the broken value's release out of the block
// body — which the break's jump to the exit label skips — and onto the `Block`
// node, emitted after that label. That placement is correct only while the
// block's result regions reach the binding naming it, so the binding-chain
// `decref_point` extension carries the release past every later read. Without
// that flow the release fires at the exit label and each read below touches
// freed pages — SIGSEGV under guardfree. Drives the broken value's heap contents
// through a post-block read for every placement (bare, `let`-bound, stored,
// branched, out of a `while`, out of a nested block, forwarded into a call) with
// a fresh subject per iteration so region ids recycle under the reader. The leak
// face is `region-break-transfer.lisp`.
#[test]
fn region_break_transfer_uaf() {
    run_elle_script_with_args(
        "region-break-transfer-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the same jump that strands the broken value strands every OTHER
// release between the break site and the exit label, and those are re-anchored
// to the block too (docs/impl/region/mechanism.md § "A release the break jumps
// over is not a release"). Moving a release later can only over-keep — while it
// still names the same value when it runs, which is what this drives: a window
// value read after the block, stored into a container, returned, captured by a
// closure, and reached across the two scopes the window stops at (a nested loop,
// whose body re-allocates per iteration, and a nested lambda, whose releases
// belong to another frame). A release hoisted out of either frees a live region
// and every read below touches freed pages — SIGSEGV under guardfree. The leak
// face is `region-break-skip.lisp`.
#[test]
fn region_break_skip_uaf() {
    run_elle_script_with_args(
        "region-break-skip-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a region live-in to a branch has ONE release, and it is anchored where
// every arm reaches it rather than inside the arm that happens to name it last
// (docs/impl/region/mechanism.md § "A release inside one arm is not a release on
// the other arms"). The release moves later, which can only over-keep — while it
// still drops the frame's own reference and no other, which is what this drives:
// an arm that stores the value into a container, hands it to a closure, returns
// it to its caller, and parks a fiber that resolves it through its own
// activation map after the branch; plus the three scopes the window stops at (a
// nested loop, whose body re-allocates per iteration, a nested lambda, whose
// releases belong to another frame, and a frame-replacing tail call, which never
// reaches the merge). Freeing a live region there faults on the read below —
// SIGSEGV under guardfree. The leak face is `region-branch-arm-window.lisp`.
#[test]
fn region_branch_arm_window_uaf() {
    run_elle_script_with_args(
        "region-branch-arm-window-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — an inlined callee's body regions name the CALLEE's activation, so the
// caller names the call's own region for the result (docs/impl/region/mechanism.md
// § "A call's result is named by the call's own region"). The result therefore
// carries exactly one caller-side release, so what the callee hands back that is
// not freshly its own has no second caller-side holding and must ride a counted
// edge instead. This drives each such
// hand-off: an argument returned unchanged, one of two arguments picked per path,
// an element read out of an argument, a result stored into a module-level
// container, captured by a closure called later, yielded across the fiber
// frontier, fed forward as the next call's argument, read past a branch merge, and
// allocated in a self-recursive walk's base case. Freeing any of them early faults
// on the read below — SIGSEGV under guardfree. The leak face is
// `region-inline-result-naming.lisp`.
#[test]
fn region_inline_result_naming_uaf() {
    run_elle_script_with_args(
        "region-inline-result-naming-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `match` arm's pattern binding records its scope, so a read of it
// inside a loop no longer reads as a read of a loop-external binding and the
// scrutinee's release stays in the body that allocates it (docs/impl/region/
// mechanism.md § "Every binder records its scope"). The release moves EARLIER —
// from after the loop to once per iteration — so what it must not do is drop a
// projection someone else still holds. This drives every hand-off out of the
// iteration: the arm stores the projection into a fn-local cell, into a
// module-level container, captures it in a closure called after the loop, breaks
// out of the loop with it, yields it across the fiber frontier, reads an inner
// loop's projection from the outer body, reads into a nested container
// projection, and feeds it back into the next iteration's scrutinee. Freeing any
// of them at the iteration's end faults on the read — SIGSEGV under guardfree.
// The leak face is `region-match-bind-loop.lisp`.
#[test]
fn region_match_bind_loop_uaf() {
    run_elle_script_with_args(
        "region-match-bind-loop-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a spliced call's args array is reclaimed by the call that consumes it
// (docs/impl/region/mechanism.md § "A spliced call's arguments come out of an
// array the convention owns"). The array counts one reference per element, so
// that reclaim is a cascade on a path that ran none before, and four faces must
// survive it: the ARGUMENT the callee still reads after the array is gone, the
// SOURCE the splice read — released ahead of the frame replacement now that a
// spliced tail call moves nothing — the pass-through RESULT handed back out of
// an argument, and an OUTER holder in call position. Every read below happens
// after the reclaim ran, so an over-release faults there — SIGSEGV under
// guardfree. The leak face is `region-splice-args.lisp`.
#[test]
fn region_splice_args_uaf() {
    run_elle_script_with_args(
        "region-splice-args-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `def` evaluates to what it bound, so its initializer's demise must
// not be narrowed onto the initializer when nothing reads the binding
// (docs/impl/region/mechanism.md § "A binder's init release lands after the slot
// store"). Every other binder's value is its BODY, so an unread init really is
// dead at the init; a `def`'s value IS the init and flows straight on. This
// drives every way it leaves — handed to a callee, returned, bound to a second
// name, propagated through a `begin`, produced by a branch arm, stored into a
// container that outlives the frame, captured by a closure, and resolved by a
// parked frame after a yield. Freeing any of them at the initializer faults on
// the read — SIGSEGV under guardfree. The leak face is
// `region-define-init-release.lisp`.
#[test]
fn region_define_init_release_uaf() {
    run_elle_script_with_args(
        "region-define-init-release-uaf",
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

// A reassigned mutable binding fed by a CALL RESULT must hold a COUNTED
// reference to it (docs/impl/region/bindings.md): the call result's own
// placeholder release fires regardless, so the 1-slot container cannot also
// donate — and the counted store must be emitted before `StoreLocal` consumes
// the value register, or the retain lands on the displaced prior instead.
#[test]
fn region_reassign_callresult_store() {
    run_elle_script_with_args(
        "region-reassign-callresult-store",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the per-path return frontier (docs/impl/region/mechanism.md § "The return
// frontier is per-path"). A returned region is the caller's to free only on the
// paths that hand it over; a branch arm that leaves without it, or one that leaves
// WITH it while a sibling arm holds the `decref_point`, still owes the callee-side
// release. Both compensations are RC-neutral only if they land on the right path:
// the dead-arm head release must not fire where the mint did, and the returning
// arm's release must follow its mint. Getting either wrong frees the value under
// the caller's read — silent on the plain tiers once the page recycles, a
// deterministic fault here. The file drives both arms of every shape past a priming
// loop and reads the result each time, so an over-free is loud and the leak face is
// pinned by the same region-count deltas. Full mechanism in the file header.
#[test]
fn region_return_arm_escape_uaf() {
    run_elle_script_with_args(
        "region-return-arm-escape-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — branch compensation reads the ARM STRUCTURE, neither the branch's kind nor
// its arity (docs/impl/region/mechanism.md § "The return frontier is per-path"). A `match` arm
// that never touches a live local owes that local's release, exactly as a two-armed
// `if`'s dead arm does. The head release is the one admitted unconditionally past
// the return frontier, so landing it on the wrong arm frees the value under the arm
// that reads it — or under the caller that was just handed it. The file drives every
// arm of every shape past a priming loop and reads each result, so an over-free
// faults deterministically here while the leak face rides the same region-count
// deltas. Full mechanism in the file header.
#[test]
fn region_match_dead_arm_uaf() {
    run_elle_script_with_args(
        "region-match-dead-arm-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
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

// Guard — a `break` opens a relocation point at the end of the block it leaves,
// and a release emitted while that block is still open is REPLICATED there
// (docs/impl/region/replicate.md). Each replica fires on a path that ran no
// release before, so it owes the same count argument the frame-exit relocation's
// replicas owe, and the value the break CARRIES is exempt — freeing it there
// would free what the block is about to hand its consumer. Under the UAF oracle
// each of those faults at the read; the harness runs the file under its vm/jit
// policies WITHOUT the oracle, where the freed page is stale but intact and the
// functional asserts pass. The leak face is
// tests/elle/region-break-loop-replica.lisp.
#[test]
fn region_break_loop_replica_uaf() {
    run_elle_script_with_args(
        "region-break-loop-replica-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
