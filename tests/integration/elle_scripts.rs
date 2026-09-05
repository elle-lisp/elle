// audited: 2026-09-05
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

mod captures {
    include!("elle_scripts/captures.rs");
}
mod containers {
    include!("elle_scripts/containers.rs");
}
mod dissolution {
    include!("elle_scripts/dissolution.rs");
}
mod fibers {
    include!("elle_scripts/fibers.rs");
}
mod frames {
    include!("elle_scripts/frames.rs");
}
mod modes {
    include!("elle_scripts/modes.rs");
}
mod tailcalls {
    include!("elle_scripts/tailcalls.rs");
}
