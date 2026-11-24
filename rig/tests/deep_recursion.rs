// audited: 2026-11-24
// A 100,000-deep recursion completes on both tiers, and the depth cap halts a
// runaway recursion and one that ends past the cap.
// docs/impl/vm.md
// docs/impl/jit-selfcall.md

mod common;

use common::{rig, stderr, stdout, Scratch};
use std::os::unix::process::ExitStatusExt;
use std::process::Output;

/// Every call runs on the interpreter's fiber frames.
const INTERPRETED: &str = "jit = \"off\"\n";

/// Every function compiled on its first call, on the VM thread, so compiled
/// code runs from the second level down without waiting on the worker.
const COMPILED: &str = "jit = \"eager\"\ntrace = [\"syncjit\"]\n";

const SUM_TO: &str = "(elle/epoch 13)\n\
                      (defn sum-to [n] (if (= n 0) 0 (+ n (sum-to (- n 1)))))\n\
                      (println (sum-to 100000))\n";

/// A recursion that never reaches its base case, under a cap low enough to
/// meet quickly.
const RUNAWAY: &str = "(elle/epoch 13)\n\
                       (vm/config-set :max-depth 50000)\n\
                       (defn down [n] (pair n (down (+ n 1))))\n\
                       (length (down 0))\n";

/// A recursion that ends, twice as deep as the cap allows. Its argument is
/// proven an integer, so the compiled tier calls each level directly
/// (docs/impl/jit-selfcall.md).
const PAST_CAP: &str = "(elle/epoch 13)\n\
                        (vm/config-set :max-depth 5000)\n\
                        (defn sum-to [n] (if (= n 0) 0 (+ n (sum-to (- n 1)))))\n\
                        (println (sum-to 10000))\n";

/// Run `program` on the rig with `sidecar` beside it.
fn run(sidecar: &str, program: &str) -> Output {
    let dir = Scratch::new("deep");
    let path = dir.write("deep.lisp", program);
    dir.write("deep.toml", sidecar);
    rig(&[path])
}

fn assert_completes(sidecar: &str) {
    let out = run(sidecar, SUM_TO);
    assert!(
        out.status.success(),
        "a non-tail recursion 100,000 deep under {sidecar:?} did not complete: \
         {:?}\nstderr:\n{}",
        out.status,
        stderr(&out)
    );
    assert_eq!(
        stdout(&out).trim(),
        "5000050000",
        "wrong sum under {sidecar:?}"
    );
}

/// A halt reaches the root as a fatal error: exit 1 with a message, never a
/// signal.
fn assert_halts(sidecar: &str, program: &str, needles: &[&str]) {
    let out = run(sidecar, program);
    let err = stderr(&out);
    assert_eq!(
        out.status.signal(),
        None,
        "the rig under {sidecar:?} was killed by signal {:?} instead of halting \
         (SIGSEGV/11 or SIGABRT/6 is the Rust stack overflowing). stderr:\n{err}",
        out.status.signal(),
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "a halt under {sidecar:?} exits 1. stderr:\n{err}"
    );
    for needle in needles {
        assert!(
            err.contains(needle),
            "stderr under {sidecar:?} lacks {needle:?}:\n{err}"
        );
    }
}

// Counter-factual: while each call nested 25–30 KB of Rust frames, the VM
// halted this at depth 200.
#[test]
fn non_tail_recursion_100000_deep_completes_interpreted() {
    assert_completes(INTERPRETED);
}

// The compiled tier nests a native frame per call, so this passes only if a
// compiled call hands its callee to the interpreter once the native stack runs
// low. Without that hand-off the recursion runs the native stack out part of
// the way down, and halts with `:stack-overflow` at the next re-entry. The
// argument is proven an integer, so each level is a direct self-call, whose
// stack guard makes the same hand-off. A direct call without the guard
// overflows the thread's stack, and Rust's overflow handler aborts the rig
// (SIGABRT).
#[cfg(feature = "jit")]
#[test]
fn non_tail_recursion_100000_deep_completes_compiled() {
    assert_completes(COMPILED);
}

#[test]
fn runaway_recursion_halts_at_the_depth_cap_interpreted() {
    assert_halts(INTERPRETED, RUNAWAY, &["stack-overflow", "50000"]);
}

#[cfg(feature = "jit")]
#[test]
fn runaway_recursion_halts_at_the_depth_cap_compiled() {
    assert_halts(COMPILED, RUNAWAY, &["stack-overflow", "50000"]);
}

#[test]
fn recursion_past_the_depth_cap_halts_interpreted() {
    assert_halts(INTERPRETED, PAST_CAP, &["stack-overflow", "5000"]);
}

// Counter-factual: a direct self-call that counted no depth. Ten thousand
// levels fit on the native stack, so the recursion ends and prints its sum;
// the runaway above halts either way, because the interpreter counts from
// where the native stack runs low.
#[cfg(feature = "jit")]
#[test]
fn recursion_past_the_depth_cap_halts_compiled() {
    assert_halts(COMPILED, PAST_CAP, &["stack-overflow", "5000"]);
}
