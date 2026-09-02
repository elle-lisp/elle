//! Counterfactual for cumulative *concurrent-compile* heap corruption.
//!
//! Multiple threads each build a fresh `Linter` — its own `Runtime`, its own
//! instance heap — and compile the stdlib + analyze concurrently, exactly what
//! `cargo test` does when it runs the `lint`/`lsp` integration tests in
//! parallel. The corruption this guards against is a within-thread
//! use-after-free: a region freed while a `Bytecode`'s strings/slices still
//! point into its pages, whose page-reuse window opens far more often under
//! multi-thread memory pressure. It surfaces as a torn `String` length inside a
//! `Bytecode` clone in `lir::emit`,
//! attempting a multi-hundred-GiB allocation that aborts (a garbage length, so
//! an OOM-abort rather than a clean SIGSEGV). It reproduces with
//! `--jit=off --mlir=off`, so it is neither the JIT nor the analyzer's HIR logic.
//!
//! The repro runs in a re-exec'd CHILD under an 8 GiB address-space rlimit so a
//! corrupted allocation aborts the child fast instead of exhausting the machine;
//! the PARENT asserts the child exited cleanly. The PARENT leg itself never
//! corrupts (it only spawns + waits), so this runs in `make test`/CI as a normal
//! test: the parent's assertion fails cleanly if the child aborts — the harness
//! is not taken down.

use std::os::unix::process::CommandExt;
use std::process::Command;

/// Argv sentinel for the re-exec'd child leg. Passed as an extra `--exact`
/// test filter: it matches no test name, so libtest ignores it, and the child
/// detects it via `std::env::args()`. An argv flag, not an environment
/// variable — env vars are banned here (harder to gate, to wire through
/// subprocesses, and for users/agents to discover than argv).
const CHILD_ARG: &str = "parallel-compile-child-leg";

fn run_parallel_repro() {
    // 8 threads × 2 iterations of (build a Linter, compile + analyze). Each
    // Linter owns its own instance heap, freed when it drops, so peak resident
    // memory stays comfortably under the child's 8 GiB cap — an abort therefore
    // means corruption (a torn length attempting a multi-hundred-GiB
    // allocation), not ordinary memory pressure.
    let handles: Vec<_> = (0..8)
        .map(|_| {
            std::thread::spawn(|| {
                for _ in 0..2 {
                    let mut linter = elle::Linter::new(elle::LintConfig::default());
                    let _ = linter.lint_str("(+ 1 2 3)", "repro.lisp");
                }
            })
        })
        .collect();
    for h in handles {
        h.join().expect("worker thread panicked");
    }
}

#[test]
fn parallel_compile_no_corruption() {
    // CHILD leg: actually run the repro. Reaching the end = no corruption.
    if std::env::args().any(|a| a == CHILD_ARG) {
        run_parallel_repro();
        return;
    }

    // PARENT leg: re-exec this test binary to run ONLY this test as the child,
    // capped at 8 GiB of address space. The child re-enters via the CHILD_ARG
    // argv sentinel (an inert extra `--exact` filter the child detects in its
    // args).
    let exe = std::env::current_exe().expect("current_exe");
    let mut cmd = Command::new(exe);
    cmd.args([
        "--exact",
        "parallel_compile_no_corruption",
        CHILD_ARG,
        "--nocapture",
        "--test-threads=1",
    ]);
    unsafe {
        cmd.pre_exec(|| {
            let lim = libc::rlimit {
                rlim_cur: 8u64 << 30,
                rlim_max: 8u64 << 30,
            };
            // Best-effort; if it fails the test still runs, just less safely.
            libc::setrlimit(libc::RLIMIT_AS, &lim);
            Ok(())
        });
    }
    let status = cmd.status().expect("spawn child repro");
    assert!(
        status.success(),
        "concurrent-compile child exited abnormally ({status:?}): the cumulative \
         parallel-compile heap corruption reproduced (torn Bytecode string \
         in lir::emit)."
    );
}
