// audited: 2026-09-23
// Guardfree pins for where a release lands: the branch-arm window, the break window, and the binder pins.
//
// docs/analysis/testing.md

use super::*;

// A fn-local mutable accumulated across a `while` and handed back takes the
// 1-slot-container model. Each value the loop displaces is released at the
// overwrite, and the last one leaves with the caller. The `Return`'s mint pays
// for the caller's reference, and the cell's content drop, emitted after that
// mint, releases the cell's (docs/impl/region/bindings.md § "Returned fn-local
// reassigned mutables — the return claims the MINT's reference, not the
// cell's"). This pin runs under the UAF oracle because the model runs a free
// path the unsuppressed baseline never ran. Were the content drop to consume the
// caller's reference instead, the returned chain would fault at the caller's
// read. The harness already covers the file's plain vm/jit runs and its
// bounded-rate face.
#[test]
fn region_loop_acc_return_uaf() {
    run_elle_script_with_args(
        "region-loop-acc-return",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a top-level mutable reassigned to a value referencing its old content
// (`(assign x (pair v x))`) must survive when the file runs as the `%file-body`
// whole-module thunk (the `elle test` shape). The thunk makes `in_lambda` true,
// so the solver reads `is_file_scope` and routes the file-letrec binding to the
// top-level container model. Classified fn-local instead, the dead
// `__file_expr_N` statement wrapper's slot-routed decref frees the just-stored
// value under the cell. This is the advanced.lisp `decision tree match in loop`
// shape.
#[test]
fn region_toplevel_reassign_thunk_uaf() {
    run_elle_script_with_args(
        "region-toplevel-reassign-thunk-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `match` pattern binding that aliases into the scrutinee's region
// (`(a & rest)`, `(h . t)`, an immutable-array element, an immutable-struct
// value) must carry the scrutinee's regions in its `binding_regions`. The subject
// region's decref_point then extends over the bound alias, so the subject is not
// freed under the consumer's borrow, which would SIGSEGV under guardfree. The
// solver's `Match` arm propagates the scrutinee's regions to each arm binding,
// mirroring its `Destructure` arm. This is the advanced.lisp `guard with rest`
// shape.
#[test]
fn region_match_rest_uaf() {
    run_elle_script_with_args(
        "region-match-rest-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a match-destructured `rest` alias (`(a & rest)`) is a BORROWED subview
// of the scrutinee, and the pattern load takes no counted reference. The lowerer
// marks such bindings borrowed (`destructure_alias_bindings`), so a call site
// passing one mints a fresh owning reference the callee's param release
// balances. Passed uncounted to an owned-param callee (tail or not), the alias
// would let that release free the caller's still-live scrutinee region —
// SIGSEGV/SIGBUS under guardfree.
#[test]
fn region_match_rest_tail_move_uaf() {
    run_elle_script_with_args(
        "region-match-rest-tail-move-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — `break` TRANSFERS its value to the enclosing block
// (docs/impl/region/anchors.md § "`break` transfers its value; it does not
// consume it"). The transfer moves the broken value's release out of the block
// body, which the break's jump to the exit label skips. The release lands on the
// node that consumes the block's value, or on the `Block` itself, after that
// label. That placement is correct only while the block's result regions reach
// the binding naming it, so the binding-chain `decref_point` extension carries
// the release past every later read. Without that flow the extension never sees
// the broken regions, and each read below touches freed pages — SIGSEGV under
// guardfree.
//
// The file reads the broken value's heap contents after the block, for every
// placement. The value is bare, `let`-bound, stored, branched, or forwarded into
// a call. It is also broken out of a `while`, a nested block, and a loop in a
// tail block. A fresh subject per iteration makes region ids recycle under the
// reader. The leak face is `region-break-transfer.lisp`.
#[test]
fn region_break_transfer_uaf() {
    run_elle_script_with_args(
        "region-break-transfer-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the same jump that strands the broken value strands every OTHER
// release between the break site and the exit label, and those are re-anchored
// to the block too (docs/impl/region/anchors.md § "A release the break jumps
// over is not a release"). Moving a release later can only over-keep, but only
// while it still names the same value when it runs. This file drives that
// condition: a window value read after the block, stored into a container,
// returned, and captured by a closure. It also drives the two scopes the window
// stops at: a nested loop, whose body re-allocates per iteration, and a nested
// lambda, whose releases belong to another frame. A release hoisted out of either
// frees a live region and every read below touches freed pages — SIGSEGV under
// guardfree. The leak face is `region-break-skip.lisp`.
#[test]
fn region_break_skip_uaf() {
    run_elle_script_with_args(
        "region-break-skip-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a region live-in to a branch has ONE release, and it is anchored where
// every arm reaches it rather than inside the arm that happens to name it last
// (docs/impl/region/window.md § "A release inside one arm is not a release on
// the other arms"). The release moves later, which can only over-keep, but only
// while it still drops the frame's own reference and no other.
//
// This file drives that condition: an arm stores the value into a container,
// hands it to a closure, or returns it to its caller. Another arm parks a fiber
// that resolves the value through its own activation map after the branch. The
// file also drives the two scopes the window stops at, a nested loop and a nested
// lambda. It also drives an arm that leaves through a frame-replacing tail call
// and never reaches the merge. Freeing a live region there faults on the read
// below — SIGSEGV under guardfree. The leak face is
// `region-branch-arm-window.lisp`.
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
// edge instead.
//
// This drives each such hand-off: an argument returned unchanged, one of two
// arguments picked per path, and an element read out of an argument. The result
// is also stored into a module-level container, captured by a closure called
// later, and yielded across the fiber frontier. It is also fed forward as the
// next call's argument, read past a branch merge, and allocated in a
// self-recursive walk's base case. Freeing any of them early faults on the read
// below — SIGSEGV under guardfree. The leak face is
// `region-inline-result-naming.lisp`.
#[test]
fn region_inline_result_naming_uaf() {
    run_elle_script_with_args(
        "region-inline-result-naming-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `match` arm's pattern binding records its scope, so a read of it
// inside a loop does not read as a read of a loop-external binding
// (docs/impl/region/anchors.md § "Every binder records its scope"). The
// scrutinee's release therefore stays in the body that allocates it and fires
// once per iteration, not after the loop. What it must not do is drop a
// projection someone else still holds.
//
// This drives every hand-off out of the iteration. The arm stores the projection
// into a fn-local cell or a module-level container, captures it in a closure
// called after the loop, breaks out with it, or yields it across the fiber
// frontier. The file also reads an inner loop's projection from the outer
// body, reads into a nested container projection, and feeds a projection into
// the next iteration's scrutinee. Freeing any of them at the iteration's end
// faults on the read — SIGSEGV under guardfree. The leak face is
// `region-match-bind-loop.lisp`.
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
// that reclaim is a cascade on a path that ran none before. Four faces must
// survive it. The callee reads the ARGUMENT after the array is gone. The SOURCE
// the splice read is released ahead of the frame replacement, because a spliced
// tail call moves nothing. The pass-through RESULT is handed back out of an
// argument, and an OUTER holder sits in call position.
//
// Every read below happens after the reclaim ran, so an over-release faults
// there — SIGSEGV under guardfree. The leak face is `region-splice-args.lisp`.
#[test]
fn region_splice_args_uaf() {
    run_elle_script_with_args(
        "region-splice-args-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a rest name owns the collection its pattern BUILT, so it carries a
// release of its own (docs/impl/region/anchors.md § "A rest pattern's
// collection is built, not read out"). This drives every way such a collection
// leaves the destructure: read in its own scope, returned, stored into a
// container that outlives the loop, or captured by a closure called afterwards.
// It is also borrowed element-wise, carried across a fiber yield, left behind by
// a failed guard, broken out of a loop, and handed to a tail call. Freeing any of
// them early faults on the read — SIGSEGV under guardfree. The leak face is
// `region-rest-pattern-slice.lisp`.
#[test]
fn region_rest_pattern_slice_uaf() {
    run_elle_script_with_args(
        "region-rest-pattern-slice-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `def` evaluates to what it bound, so its initializer's demise must
// not be narrowed onto the initializer when nothing reads the binding
// (docs/impl/region/anchors.md § "A binder's init release lands after the slot
// store"). Every other binder's value is its BODY, so an unread init really is
// dead at the init; a `def`'s value IS the init and flows straight on. This
// drives every way it leaves: handed to a callee, returned, bound to a second
// name, propagated through a `begin`, or produced by a branch arm. It is also
// stored into a container that outlives the frame, captured by a closure, and
// resolved by a parked frame after a yield. Freeing any of them at the
// initializer faults on the read — SIGSEGV under guardfree. The leak face is
// `region-define-init-release.lisp`.
#[test]
fn region_define_init_release_uaf() {
    run_elle_script_with_args(
        "region-define-init-release-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a whole-value read of a REASSIGNED CAPTURED CELL (fn-local upvalue
// read AND module-scope `def @cell`) must take a counted reference. Otherwise
// the cell's next overwrite (`capture_store_with_rebind` decrefs the displaced
// prior unconditionally) frees the value under the reader — SIGSEGV under
// guardfree. The reader takes Rule 5's "new reference" pass-through: an
// `IncrefValueRegion` at the read, balanced by the `DecrefValueRegion` at its
// last use. The fn-local shape is `(let [batch ready] (assign ready @[]) …)`,
// with `ready` a local of an enclosing fn (docs/impl/region/cells.md
// § "Captured reassigned cells").
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
// while it is still `Owned`, so it must be emitted before the container's subtree drop
// (docs/impl/region/adopt.md § "The lifetime obligation the root carries"). At the
// let-body the pair and the container share a `decref_point`. Two releases can free
// the container there: its binding release, and the discarded pass-through result of
// `%array-push`, which returns its container. Ordered after those, the pair's plain
// `DecrefRegion` runs after the drop reclaimed the pair — a phantom/double-free,
// SIGSEGV under guardfree. The topological release order over the adopt edge
// (`order_releases`, called from `with_region_info`; member → owner) keeps the
// member's release ahead of the container's.
#[test]
fn region_array_push_pair_loop_uaf() {
    run_elle_script_with_args(
        "region-array-push-pair-loop-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the splice/`apply` manifestation of the native-tail-return retain.
// `(first ;argv)` lowers to `TailCallArrayMut`, whose native-completion
// fall-through emits the ReturnValue retain (`lower_spliced_call`). The call
// reclaims the args array, so without that retain the caller's
// `DecrefValueRegion` drains the native's one pass-through reference and the read
// faults under guardfree. The trap: a stranded args array would still hold the
// result, and the asserts would pass with the retain missing. So this pin is a
// witness only while tests/elle/region-splice-args.lisp stays bounded. The
// non-splice twin is `region_native_tail_return_uaf` in
// tests/integration/elle_scripts/tailcalls.rs.
#[test]
fn region_splice_tail_return() {
    run_elle_script_with_args(
        "region-splice-tail-return",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// A reassigned mutable binding fed by a CALL RESULT must hold a COUNTED
// reference to it (docs/impl/region/bindings.md). The call result's own
// placeholder release fires regardless, so the 1-slot container cannot also
// donate. The counted store must be emitted before `StoreLocal` consumes the
// value register, or the retain lands on the displaced prior instead.
#[test]
fn region_reassign_callresult_store() {
    run_elle_script_with_args(
        "region-reassign-callresult-store",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — the per-path return frontier (docs/impl/region/compensate.md § "The return
// frontier is per-path"). A returned region is the caller's to free only on the
// paths that hand it over. A branch arm that leaves without it, or one that leaves
// WITH it while a sibling arm holds the `decref_point`, still owes the callee-side
// release. Both compensations are RC-neutral only if they land on the right path:
// the dead-arm head release must not fire where the mint did, and the returning
// arm's release must follow its mint. Getting either wrong frees the value under
// the caller's read — silent on the plain tiers while the freed page stays intact,
// a deterministic fault here. The file drives both arms of every shape past a
// priming loop and reads the result each time, so an over-free is loud and the leak
// face is pinned by the same object-count deltas.
#[test]
fn region_return_arm_escape_uaf() {
    run_elle_script_with_args(
        "region-return-arm-escape-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — branch compensation reads the ARM STRUCTURE, neither the branch's kind nor
// its arity (docs/impl/region/compensate.md § "The return frontier is per-path"). A `match` arm
// that never touches a live local owes that local's release, exactly as a two-armed
// `if`'s dead arm does. The head release is the one admitted past the return
// frontier without a funding retain. Landed on the wrong arm, it frees the value
// under the arm that reads it, or under the caller that was just handed it. The file
// drives every arm of every shape past a priming loop and reads each result, so an
// over-free faults deterministically here while the leak face rides the same
// object-count deltas.
#[test]
fn region_match_dead_arm_uaf() {
    run_elle_script_with_args(
        "region-match-dead-arm-leak",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a `@`-mutable parameter reassigned in its body, whose (post-reassign)
// value is MOVED into a tail call. The param is materialized as a capture cell its
// own function owns. The tail-call move must retain the moved value (the
// borrowed-tail-arg retain) BEFORE the param cell's last-use `DecrefCellRegion`,
// whose cascade frees the cell's contents — the very value being moved. Emitting the
// cell release first frees the moved value, and the retain then reads a freed page.
// `lower_call` defers the tail arg's decrefs so the retain orders ahead of them.
//
// This pin runs under guardfree so a regression faults deterministically at that
// stale read. The harness runs the file under its vm/jit policies WITHOUT the
// oracle, where the freed page is stale-but-intact and the functional asserts pass.
// The file also drives an overwrite-return, a multi-reassign chain, an aliased-arg
// clobber, and an id-recycling loop.
#[test]
fn region_mutable_reassign_param_uaf() {
    run_elle_script_with_args("region-mutable-reassign-param", &["--trace=guardfree"]);
}

// Guard — a `break` opens a relocation point at the end of the block it leaves,
// and a release emitted while that block is still open is REPLICATED there
// (docs/impl/region/replicate.md). Each replica fires on a path that ran no
// release before, so it owes the same count argument the frame-exit relocation's
// replicas owe. The value the break CARRIES is exempt, because freeing it there
// would free what the block is about to hand its consumer. Under the UAF oracle a
// wrong replica faults at the read. The harness runs the file under its vm/jit
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
