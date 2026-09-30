// audited: 2026-09-29
//! elle's subcommands — `fmt`, `lint`, `lsp`, `rewrite`, `image`, `semver`, `test` — dispatched for `elle` and the rig alike.
//!
//! rig/overview.md
//! docs/test-runner.md
//!
//! A program can run its own executable with a subcommand, as the semver
//! tool's tests do with `(elle/executable)`. Under the rig that executable is
//! `elle-rig`, so both binaries answer the subcommands here, and a program runs
//! under the rig unchanged.

use crate::runtime::Runtime;

/// The agent-first test runner, embedded at build time, in the order its
/// definitions are evaluated. See docs/test-runner.md.
const TEST_RUNNER_FRAGMENTS: &[&str] = &[
    include_str!("../test/store.lisp"),
    include_str!("../test/import.lisp"),
    include_str!("../test/exec.lisp"),
    include_str!("../test/record.lisp"),
    include_str!("../test/view.lisp"),
    include_str!("../test/main.lisp"),
];

/// The runner's flags that take values (src/test/main.lisp).
const RUNNER_VALUE_FLAGS: &[(&str, usize)] = &[
    ("--db", 1),
    ("--timeout", 1),
    ("--wide", 1),
    ("--wide-timeout", 1),
    ("--isolate", 1),
    ("--host", 1),
    ("--corpus", 1),
    ("--import", 1),
    ("--query", 1),
    ("--promote", 2),
];

/// Run the subcommand `args` names, and answer the process exit code; `None`
/// when `args` names no subcommand. `args` is the command line after the
/// executable's own name.
pub fn subcommand(args: &[String]) -> Option<i32> {
    let (name, rest) = args.split_first()?;
    Some(match name.as_str() {
        "fmt" => crate::formatter::run::run(rest),
        "lint" => crate::lint::run::run(rest),
        "lsp" => crate::lsp::run::run(),
        "rewrite" => crate::rewrite::run::run(rest),
        // A boot image is written by a full source boot, so this needs a
        // `Runtime` like `test` does.
        "image" => super::image::run(rest),
        // The gate is an Elle program (src/semver) and needs a full VM.
        "semver" => super::semver::run(rest.to_vec()),
        // The runner is an Elle program (src/test). Unlike fmt and lint it
        // needs a full VM (sqlite FFI, stdlib, threads).
        "test" => run_test(rest.to_vec()),
        _ => return None,
    })
}

/// Split a subcommand's argv into elle's own flags — `--trace=`, `--boot-image=`
/// and `--dump=stats`, which configure the VM the subcommand runs on — and the
/// subcommand's. Each `(flag, n)` in `takes_values` keeps the `n` arguments after
/// it, whatever they spell.
pub(super) fn split_own_flags(
    args: Vec<String>,
    takes_values: &[(&str, usize)],
) -> (Vec<String>, Vec<String>) {
    let mut own = Vec::new();
    let mut rest = Vec::new();
    let mut args = args.into_iter();
    while let Some(arg) = args.next() {
        if let Some((_, n)) = takes_values.iter().find(|(flag, _)| *flag == arg) {
            rest.push(arg);
            rest.extend(args.by_ref().take(*n));
        } else if arg.starts_with("--trace=")
            || arg.starts_with("--boot-image=")
            || arg == "--dump=stats"
        {
            own.push(arg);
        } else {
            rest.push(arg);
        }
    }
    (own, rest)
}

/// The runner as one module.
///
/// Each fragment is a whole Elle file — formatted, stamped, and read on its
/// own — so each carries the epoch declaration a file needs. A module declares
/// its epoch once, so every fragment past the first drops the line here.
fn test_runner_source() -> String {
    let mut src = String::new();
    for (i, fragment) in TEST_RUNNER_FRAGMENTS.iter().enumerate() {
        for line in fragment.lines() {
            if i > 0 && line.starts_with("(elle/epoch") {
                continue;
            }
            src.push_str(line);
            src.push('\n');
        }
    }
    src
}

/// `elle test ...` — set up a full VM and run the embedded runner with the
/// post-`test` arguments exposed to it as the program argv (via `(sys/argv)`).
/// The runner calls `(os/exit ...)` itself with the gate code; the Ok/Err
/// mapping here is the fallback if it returns without exiting.
fn run_test(sub_args: Vec<String>) -> i32 {
    // Split off elle's own flags so the embedded runner's VM (and the off-VM
    // free-log / page-claim histogram) honour them; the rest become the
    // runner's argv. A runner flag's value is the runner's even when it spells
    // one of elle's flags: `--isolate '--trace=scrub'` hands the flag to each
    // child, not to this VM. `--boot-image=` boots this instance from an image,
    // which is how a suite is compiled against a hydrated stdlib
    // (docs/impl/image/boot.md).
    let (config_flags, sub_args) = split_own_flags(sub_args, RUNNER_VALUE_FLAGS);
    let (mut config, _rest) = crate::config::Config::parse(&config_flags).unwrap_or_else(|e| {
        eprintln!("elle test: {}", e);
        std::process::exit(1);
    });
    // The runner's workers run a whole-file script with the JIT off and with it
    // eager, and this is the one process that may set either
    // (docs/test-runner.md).
    config.test_runner = true;
    crate::config::init(config);
    crate::io::init_process_signals();

    // One runtime, one teardown — the same lifecycle every entry path uses.
    let mut rt = Runtime::new();
    rt.vm().source_arg = "<test>".to_string();
    rt.vm().user_args = sub_args;

    let code = {
        let (vm, symbols, cctx) = rt.parts();
        match super::run_source(&test_runner_source(), "src/test", vm, symbols, cctx) {
            Ok(_) => 0,
            Err(_) => 1,
        }
    };
    // The runner usually calls `(os/exit …)` itself (skipping Drop); on a
    // graceful return `rt`'s Drop runs the principled teardown sweep.
    code
}
