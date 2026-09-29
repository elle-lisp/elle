// audited: 2026-09-29
// A recursion 100,000 deep completes on the build as shipped, and each limit on
// depth halts the process with a message instead of killing it.
// docs/impl/vm.md
//
// The same recursions with the JIT off and with every function compiled on its
// first call run on the rig, in rig/tests/deep_recursion.rs.

use std::os::unix::process::ExitStatusExt;
use std::process::{Command, Output};

/// Run `program` through `elle -e`.
fn elle(program: &str) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elle"))
        .arg("-e")
        .arg(program)
        .output()
        .expect("spawn elle")
}

/// A halt reaches the root as a fatal error: exit 1 with a message, never a
/// signal. `needles` must all appear in stderr.
fn assert_halts(program: &str, needles: &[&str]) {
    let out = elle(program);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert_eq!(
        out.status.signal(),
        None,
        "elle was killed by signal {:?} instead of halting (SIGSEGV/11 or \
         SIGABRT/6 is the Rust stack overflowing). stderr:\n{stderr}",
        out.status.signal(),
    );
    assert_eq!(out.status.code(), Some(1), "a halt exits 1. stderr:\n{stderr}");
    for needle in needles {
        assert!(stderr.contains(needle), "stderr lacks {needle:?}:\n{stderr}");
    }
}

// The build decides where each call runs: the recursion starts interpreted
// and its callee compiles once it is hot, part of the way down.
#[test]
fn non_tail_recursion_100000_deep_completes() {
    let out = elle(
        "(defn sum-to [n] (if (= n 0) 0 (+ n (sum-to (- n 1))))) \
         (println (sum-to 100000))",
    );
    assert!(
        out.status.success(),
        "a non-tail recursion 100,000 deep did not complete: {:?}\nstderr:\n{}",
        out.status,
        String::from_utf8_lossy(&out.stderr)
    );
    assert_eq!(String::from_utf8_lossy(&out.stdout).trim(), "5000050000");
}

#[test]
fn runaway_recursion_halts_at_the_depth_cap() {
    assert_halts(
        "(vm/config-set :max-depth 50000) \
         (defn down [n] (pair n (down (+ n 1)))) \
         (length (down 0))",
        &["stack-overflow", "50000"],
    );
}

// `arena/allocs` runs its thunk by re-entering the VM from Rust, so this
// recursion nests Rust frames at every level whatever the tier, and the depth
// cap is far away. Counter-factual: with no native-stack check at the
// re-entry, the thread overflows and the process dies of SIGSEGV.
#[test]
fn reentrant_recursion_halts_before_the_native_stack_overflows() {
    assert_halts(
        "(defn nest [n] (+ 1 (first (arena/allocs (fn [] (nest (+ n 1))))))) \
         (nest 0)",
        &["stack-overflow", "native stack"],
    );
}
