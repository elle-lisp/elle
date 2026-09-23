// audited: 2026-09-22
// A recursion 100,000 deep completes on each tier, and each limit on depth
// halts the process with a message instead of killing it.
// docs/impl/vm.md

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Output};

/// Run `program` through `elle -e` with `flags` before it.
fn elle(flags: &[&str], program: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elle"))
        .args(flags)
        .arg("-e")
        .arg(program)
        .output()
        .expect("spawn elle")
}

/// The JIT off: every call runs on the interpreter's fiber frames.
const INTERPRETED: &[&str] = &["--jit=off"];

/// Every function compiled on its first call, on the VM thread, so compiled
/// code runs from the second level down without waiting on the worker.
const COMPILED: &[&str] = &["--jit=eager", "--trace=syncjit"];

const SUM_TO: &str = "(defn sum-to [n] (if (= n 0) 0 (+ n (sum-to (- n 1))))) \
                      (println (sum-to 100000))";

fn assert_completes(flags: &[&str]) {
    let out = elle(flags, SUM_TO);
    assert!(
        out.status.success(),
        "a non-tail recursion 100,000 deep under {flags:?} did not complete: \
         {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(
        String::from_utf8_lossy(&out.stdout).trim(),
        "5000050000",
        "wrong sum under {flags:?}"
    );
}

/// A halt reaches the root as a fatal error: exit 1 with a message, never a
/// signal. `needles` must all appear in stderr.
fn assert_halts(flags: &[&str], program: &str, needles: &[&str]) {
    let out = elle(flags, program);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.signal(),
        None,
        "elle under {flags:?} was killed by signal {:?} instead of halting \
         (SIGSEGV/11 or SIGABRT/6 is the Rust stack overflowing). stderr:\n{stderr}",
        out.status.signal(),
    );
    assert_eq!(
        out.status.code(),
        Some(1),
        "a halt under {flags:?} exits 1. stderr:\n{stderr}"
    );
    for needle in needles {
        assert!(
            stderr.contains(needle),
            "stderr under {flags:?} lacks {needle:?}:\n{stderr}"
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
// low. Without that hand-off the process dies of SIGSEGV part of the way down.
#[test]
fn non_tail_recursion_100000_deep_completes_compiled() {
    assert_completes(COMPILED);
}

/// A recursion that never reaches its base case, under a cap low enough to
/// meet quickly.
const RUNAWAY: &str = "(vm/config-set :max-depth 50000) \
                       (defn down [n] (pair n (down (+ n 1)))) \
                       (length (down 0))";

#[test]
fn runaway_recursion_halts_at_the_depth_cap_interpreted() {
    assert_halts(INTERPRETED, RUNAWAY, &["stack-overflow", "50000"]);
}

#[test]
fn runaway_recursion_halts_at_the_depth_cap_compiled() {
    assert_halts(COMPILED, RUNAWAY, &["stack-overflow", "50000"]);
}

// `arena/allocs` runs its thunk by re-entering the VM from Rust, so this
// recursion nests Rust frames at every level whatever the tier, and the depth
// cap is far away. Counter-factual: with no native-stack check at the
// re-entry, the thread overflows and the process dies of SIGSEGV.
#[test]
fn reentrant_recursion_halts_before_the_native_stack_overflows() {
    let program = "(defn nest [n] (+ 1 (first (arena/allocs (fn [] (nest (+ n 1))))))) \
                   (nest 0)";
    assert_halts(INTERPRETED, program, &["stack-overflow", "native stack"]);
}
