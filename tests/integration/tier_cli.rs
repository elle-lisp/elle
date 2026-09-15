// audited: 2026-09-14
// The tier policy the binary starts from, read back out of a running VM.
//
// src/config/tests.rs pins what `Config::parse` builds from the argv. That is
// a different question from what the binary does: `main` resolves the config,
// installs it into the process cell, and each VM seeds its RuntimeConfig from
// there. A default that never reaches the dispatcher reads correctly in the
// struct and compiles nothing, which is the gap these tests close.
//
// docs/config.md

use std::process::Command;

fn elle() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Run `source` under `args` and return its trimmed stdout.
fn run(args: &[&str], source: &str) -> String {
    let mut cmd = Command::new(elle());
    for a in args {
        cmd.arg(a);
    }
    let out = cmd.arg("-e").arg(source).output().expect("spawn elle");
    assert!(
        out.status.success(),
        "elle {:?} exited {:?}\nstderr: {}",
        args,
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).trim().to_string()
}

/// The policy a VM reports for `key` when the binary is given `args`.
fn policy(key: &str, args: &[&str]) -> String {
    run(args, &format!("(println (vm/config {key}))"))
}

#[test]
fn the_binary_starts_the_jit_adaptive() {
    assert_eq!(policy(":jit", &[]), "adaptive");
}

#[test]
fn a_jit_flag_still_reaches_the_vm() {
    assert_eq!(policy(":jit", &["--jit=off"]), "off");
    assert_eq!(policy(":jit", &["--jit=eager"]), "eager");
}

#[test]
fn the_binary_starts_mlir_off() {
    assert_eq!(policy(":mlir", &[]), "off");
}

#[test]
#[cfg(feature = "jit")]
fn a_hot_function_compiles_with_no_flag_at_all() {
    // The counter-factual the keyword alone cannot give: a policy the
    // dispatcher never consults still answers `:adaptive`. Drive a function
    // past the threshold and ask whether native code exists for it.
    //
    // The trap: compilation runs on the `elle-jit` worker, so `jit?` races it.
    // `--trace=syncjit` compiles on the VM thread instead, which is what makes
    // the answer deterministic rather than usually-true.
    let out = run(
        &["--trace=syncjit"],
        "(defn hot [n] (+ n 1)) \
         (def @i 0) (while (< i 50) (hot i) (assign i (+ i 1))) \
         (println (jit? hot))",
    );
    assert_eq!(
        out, "true",
        "50 calls with no flag must leave native code behind"
    );
}

#[test]
#[cfg(feature = "jit")]
fn jit_off_leaves_the_same_function_interpreted() {
    // The other half of the pair. Without it, a `jit?` that answered `true`
    // unconditionally would pass the test above.
    let out = run(
        &["--trace=syncjit", "--jit=off"],
        "(defn hot [n] (+ n 1)) \
         (def @i 0) (while (< i 50) (hot i) (assign i (+ i 1))) \
         (println (jit? hot))",
    );
    assert_eq!(out, "false", "--jit=off must compile nothing");
}
