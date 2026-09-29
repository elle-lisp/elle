// audited: 2026-09-29
// Scripts pinned to a JIT or backend toggle rather than to the guardfree oracle.
//
// docs/analysis/testing.md

use super::*;

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
// letrec binding-scope drop (non-tail body) or by the tail-call deferred release at the
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

// The same file under `--jit=eager`. The fixture's `-jit` driver shapes are
// JIT-admissible (their `fiber/new` lives in a helper, so the recursive
// resume caller itself compiles), so this pin drives a compiled
// `fiber/resume` caller 20000 deep — the depth a per-level Rust frame
// residue would turn into a stack-overflow abort. See the fixture header.
#[test]
fn fiber_deep_nesting_jit() {
    run_elle_file_with_args(
        "tests/integration/fixtures/fiber-depth.lisp",
        &["--jit=eager"],
    );
}

// A parked activation resumes without tripping the uncounted-borrow guard
// (src/vm/core/resume.rs → `first_stale_borrow`,
// docs/impl/region/generations.md § "Uncounted-borrow check").
//
// The trap: the guard is DEBUG-ONLY. Release compiles it out, so this file
// passes CI's release corpus whatever the region map holds; only the debug
// cargo-test profile runs it armed. That is why the pin lives here and not in
// the corpus.
//
// The counter-factual: an activation's region map is cleared by the slot-based
// `DecrefRegion` alone, so a region freed any other way leaves its entry behind
// and the physical id it names is recycled. Snapshot a parked entry at the id's
// CURRENT generation instead of the one the slot was established at, and the
// leftover reads as a live borrow of an incarnation the activation never owned.
// signals.lisp is the file that gets there: its cumulative squelch/silence/yield
// churn recycles ids fast enough, and the state it needs does not minimize to a
// standalone form.
#[test]
fn signals_no_stale_suspended_frame_region_borrow() {
    run_elle_script_with_args("signals", &["--jit=off"]);
}
