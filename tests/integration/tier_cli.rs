// audited: 2026-10-21
// The tier a build starts, read back out of a running VM, and the tier flags a
// user build refuses.
//
// docs/config.md
//
// src/config/tests.rs pins what `Config::parse` builds from the argv. That is
// a different question from what the binary does: `main` resolves the config,
// installs it into the process cell, and each VM seeds its RuntimeConfig from
// there. Whether the dispatcher compiles at that threshold needs a call that
// stays a call, which only the rig can promise, so
// tests/impl/jit-threshold.lisp holds that half.

use std::process::{Command, Output};

fn elle() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn spawn(args: &[&str], source: &str) -> Output {
    Command::new(elle())
        .args(args)
        .arg("-e")
        .arg(source)
        .output()
        .expect("spawn elle")
}

/// Run `source` under `args` and return its trimmed stdout.
fn run(args: &[&str], source: &str) -> String {
    let out = spawn(args, source);
    assert!(
        out.status.success(),
        "elle {:?} exited {:?}\nstderr: {}",
        args,
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

#[test]
#[cfg(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm")))]
fn the_default_build_reports_the_jit_threshold() {
    assert_eq!(run(&[], "(println (vm/config :jit))"), "10");
}

#[test]
#[cfg(not(all(feature = "mlir", not(feature = "wasm"))))]
fn a_build_without_mlir_reports_no_mlir_threshold() {
    assert_eq!(run(&[], "(println (vm/config :mlir))"), "nil");
}

#[test]
#[cfg(all(feature = "jit", not(feature = "mlir"), not(feature = "wasm")))]
fn a_program_reads_back_the_threshold_it_sets() {
    // The threshold is the one JIT setting a program owns.
    assert_eq!(
        run(&[], "(vm/config-set :jit 20) (println (vm/config :jit))"),
        "20"
    );
}

#[test]
fn a_program_cannot_turn_the_jit_off() {
    // The JIT compiles adaptively; no program chooses another policy. Only the
    // `elle test` process may turn a tier off or make it eager, any other
    // keyword is the wrong type, and a count below one names no threshold
    // (docs/config.md).
    for policy in [":off", ":eager"] {
        assert_eq!(
            run(
                &[],
                &format!(
                    "(let [[ok? err] (protect (vm/config-set :jit {policy}))] \
                       (println ok? \" \" (get err :error)))"
                )
            ),
            "false argument-error",
            "a program outside elle test must be refused {policy}"
        );
    }
    assert_eq!(
        run(
            &[],
            "(let [[ok? err] (protect (vm/config-set :jit :later))] \
               (println ok? \" \" (get err :error)))"
        ),
        "false type-error"
    );
    assert_eq!(
        run(
            &[],
            "(let [[ok? err] (protect (vm/config-set :jit 0))] \
               (println ok? \" \" (get err :error)))"
        ),
        "false argument-error"
    );
}

/// A user build has no flag that chooses a tier, a backend or a pass
/// (docs/config.md). It refuses each such flag by name before anything runs.
/// The counter-factual: a flag that stopped parsing would become the program's
/// name, and the run would read a missing file called `--jit=off`.
#[test]
fn a_removed_flag_is_refused_by_name() {
    for flag in [
        "--jit=off",
        "--jit=eager",
        "--mlir=off",
        "--anf=off",
        "--no-uring",
        "--stats",
        "--flip=on",
    ] {
        let out = spawn(&[flag], "(println :ran)");
        let stderr = String::from_utf8_lossy(&out.stderr);
        assert!(
            !out.status.success(),
            "elle {flag} must refuse to run; stdout:\n{}",
            String::from_utf8_lossy(&out.stdout)
        );
        assert!(
            stderr.contains("unknown option") && stderr.contains(flag),
            "elle {flag} must name the option it refused, got:\n{stderr}"
        );
    }
}

#[test]
#[cfg(not(feature = "wasm"))]
fn the_wasm_flag_is_refused_without_the_backend() {
    let out = spawn(&["--wasm=full"], "(println :ran)");
    assert!(!out.status.success());
    assert!(String::from_utf8_lossy(&out.stderr).contains("unknown option"));
}

#[test]
fn dump_stats_runs_the_program_and_reports_at_its_end() {
    let out = spawn(&["--dump=stats"], "(println :ran)");
    let stdout = String::from_utf8_lossy(&out.stdout);
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "stderr:\n{stderr}");
    assert_eq!(stdout.trim(), "ran", "the program ran");
    assert!(
        stderr.contains("[stats] live regions after teardown"),
        "--dump=stats prints the teardown census, got:\n{stderr}"
    );
}
