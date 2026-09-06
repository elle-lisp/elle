// audited: 2026-09-05
// Guardfree pins for the frame-exit relocation and the deferred channels a tail call rides.
//
// docs/analysis/testing.md

use super::*;

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

// Guard — everything the lowerer emits after a `TailCall` runs only on the NATIVE
// fall-through, so a release landing there is carried back ahead of the call
// (docs/impl/region/mechanism.md § "A release past a frame-replacing tail call is
// not a release"). This is the one release the region system moves EARLIER, and
// its legality is entirely the exemption: only a region the call itself cannot
// reach may move. This drives what the call CAN reach — an argument moved into
// the callee, a moved argument beside a hoisted sibling, the per-call callee
// closure the new activation takes over, a value the callee reads through its
// captured environment, a mutable accumulator the callee fills, a value already
// stored into a longer-lived container, a value returned through the callee, and
// an argument a parked frame resolves after the resume. Releasing any of them
// early faults on the read below — SIGSEGV under guardfree. The leak face is
// `region-tail-frame-exit.lisp`.
#[test]
fn region_tail_frame_exit_uaf() {
    run_elle_script_with_args(
        "region-tail-frame-exit-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a native tail call that leaves by a SIGNAL consumes the borrowed
// argument retains the abandoned post-`TailCall` block would have consumed
// (docs/impl/region/mechanism.md § "What the fall-through owes, a signal exit
// owes too"). Each is a release on a path that ran none before, so three faces
// must survive it: the value the signal PAYLOAD carries (a fiber carrier hands
// over its own fiber argument), the REPLAY of a parked or restarted frame that
// reaches the same release a second time, and an OUTER holder the caught error
// returns to. Every read below happens after the exit ran, so an over-release
// faults there — SIGSEGV under guardfree. The leak face is
// `region-tail-signal-exit.lisp`.
#[test]
fn region_tail_signal_exit_uaf() {
    run_elle_script_with_args(
        "region-tail-signal-exit-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a deferred tail-call release runs at every end its activation can
// reach, not only the clean break (docs/impl/region/owner.md § "A deferred
// tail-call release has the node's life"). Each is a decref the activation
// genuinely owed, now run where it never ran, so what must survive is every
// reference the activation does not own: the error PAYLOAD the raiser reached
// through the tail callee's captured environment, a closure a CONTAINER still
// holds, the RESUMED body whose completion the release belongs to, a DROPPED
// fiber's parked payload, and a tail RECURSION that re-enters with one closure
// and owes it one release. Every read happens after the exit ran, so an
// over-release faults there — SIGSEGV under guardfree. The leak face is
// `region-tail-deferred-exits.lisp`.
#[test]
fn region_tail_deferred_exits_uaf() {
    run_elle_script_with_args(
        "region-tail-deferred-exits-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// Guard — a local mutual-recursion clique (`ev`/`od`) whose `letrec` body ends in a
// tail call to a NON-member (a native `%add`, the redefined-closure operator `+`, a
// foreign fn `g`, and a MIXED member+non-member `if`) must reclaim its merged arena
// soundly. The frame-replacing `TailCall` strands the arena's binding-scope drop, so
// a closure callee rides the explicit arena adopt (`TailCall::deferred_release_slot`) at
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

// A `moves_out` REMOVE (`%pop`) returns a HEAP element that was pushed into a
// LOCAL OWNED container via a funnel. `(%array-push a (list …))` on a local `@[]`
// the ownership forest made Owned emits an `AdoptRegion` moving the list into `a`'s
// Owned subtree (RC frozen). `%pop` moving it back out must EXTRACT it — un-record
// the container edge and move it `Owned → Counted(1)` (`extract_owned_region`) — or
// the list stays interior and `a`'s subtree drop frees it while the returned Value
// still points into it (a stale-region-deref UAF; state-dependent, so the fixture
// primes id churn then drives the raw tail-pop loop). Separately, the native-tail
// path's ReturnValue `IncrefValueRegion` over the moved-out element is redundant
// (the element already carries its one caller reference), so it is suppressed for a
// moves_out ∩ PassThrough site (`RegionInfo::moves_out_release_sites`) — without
// that, tail `%pop` leaks 1 region/op. Green proves both: no over-free and no
// per-op growth. Full repro in the fixture header.
#[test]
fn region_pop_tail_moves_out_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-pop-tail-moves-out-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the tail-call move is one reference per OCCURRENCE, not one per call
// (docs/impl/region/rules.md Rule 5). A tail call pure-moves its arguments, and the
// frame holds ONE reference to a region while the callee releases once per PARAMETER,
// so an argument list naming the same region twice — `concat`'s
// `(concat-seq a rest a false)`, or two aliased bindings — hands over one reference
// against two releases and the second zeroes it under the caller's live value. Only the
// first owned occurrence is funded by the move; later ones are minted as a borrowed
// argument is, and the fixture also samples steady-state growth so a mint that is never
// consumed reads as a leak rather than passing. Full mechanism in the fixture header.
#[test]
fn region_tail_repeated_arg_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-tail-repeated-arg-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the variadic TAIL-FORWARD reference balance. Forwarding a heap value into a
// `& rest` variadic through a tail call builds the callee env as a MOVE
// (`own_params = false`): the caller's owning reference transfers, but a rest arg
// lives in the collected rest-list (its own `alloc_obj` incref), so the moved-in
// reference is surplus and must be released (`args_to_list`'s caller in `vm::env`),
// applied only to a value appearing exactly once across all arg positions. Under-
// release leaks (the `store-wrapper` oracle probe); OVER-release (an aliased/borrowed
// arg) faults under guardfree once the freed page recycles. This drives both the
// minimal forward and the stdlib-`put` store-wrapper shape past a priming loop, then
// asserts region-count bounded — so a regression in either direction is loud. Full
// mechanism in the fixture header.
#[test]
fn region_variadic_tail_forward_uaf() {
    run_elle_file_with_args(
        "tests/integration/fixtures/region-variadic-tail-forward-uaf.lisp",
        &["--jit=off", "--trace=guardfree"],
    );
}

// Guard — the BORROWED tail-arg retain must see THROUGH a branch/phi. A borrowed
// upvalue hidden behind an `(or borrowed fresh)` (or any `and`/`if`/`cond`/`match`)
// passed as a tail-call argument to an owned-param callee must still be recognized
// as borrowed, so the callee is handed a fresh owning reference instead of pure-
// moving the capture. `tail_arg_is_borrowed` (src/lir/lower/control.rs) sees a bare
// `Var`/`DerefCell(Var)` upvalue; a naive predicate returns false for an `Or` node,
// so the borrowed short-circuit operand is pure-moved and the owned-param callee's
// release drains the capture RC to a premature free (SIGSEGV under guardfree,
// `DecrefValueRegion of struct … context UpdateCapture`). The retain and the operand
// releases are value-gated, so a single retain balances BOTH arms; the fixture's
// subjects B/C guard that balance from below (no over-free on the borrow arm's
// mutable-store escape) and above (no over-incref leak when the FRESH arm is taken).
// Canonical shape: tests/elle/region-or-tail-move-borrow-uaf.lisp (the phi sibling of
// region-tail-move-borrow-uaf.lisp), plus the faithful `(protect (te (or state @{})))`
// form. Runs under guardfree so a regression faults deterministically.
#[test]
fn region_or_tail_move_borrow_uaf() {
    run_elle_script_with_args(
        "region-or-tail-move-borrow-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}

// The CONST sibling of region_tail_move_borrow_uaf: a compile-time-constant HEAP
// value — a stdlib-export closure (`+`/`inc`/`map`), a primitive's closure value,
// a `begin-for-syntax` value — reads as `LoadConst` from `immutable_values`
// (never captured; hir/analyze/scopes.rs skips the capture for a known-constant
// binding), so the frame owns NO reference to it. Tail-moving it into an
// owned-param callee lets the callee's release drain the stdlib env's region rc
// by one per call to a premature free: user-reachable as `(defn f [xs] (map inc
// xs))` — a handful of calls frees `inc`'s region under the live stdlib env
// (SIGSEGV under guardfree, tag/object-mismatch panic without). GREEN since
// `arg_leaf_is_borrowed` treats a constant HEAP value as borrowed (one fresh
// owning reference, consumed by the callee's release); the fixture's witness (c)
// guards the balance from above (no over-incref leak). Canonical shape:
// tests/elle/region-const-tail-move-borrow-uaf.lisp.
#[test]
fn region_const_tail_move_borrow_uaf() {
    run_elle_script_with_args(
        "region-const-tail-move-borrow-uaf",
        &["--jit=adaptive", "--mlir=off", "--trace=guardfree"],
    );
}
