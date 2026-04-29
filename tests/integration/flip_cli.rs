// `--flip=on` CLI surface tests.
//
// With `--flip=on`, the lowerer post-processes each function to insert
// FlipEnter at entry, FlipSwap before every tail call, and FlipExit
// before every Return. These tests verify:
//   1. Injection happens only when the flag is set.
//   2. Programs that were correct before `--flip=on` stay correct — the
//      Flip instructions are semantically equivalent to the trampoline's
//      implicit rotation.
//
// We observe (1) via `--dump=lir` and (2) by running a small tail-
// recursive loop that would be memory-unsafe under broken rotation.

use std::process::Command;

fn elle() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn run(args: &[&str], source: &str) -> (String, String, std::process::ExitStatus) {
    let mut cmd = Command::new(elle());
    for a in args {
        cmd.arg(a);
    }
    cmd.arg("-e").arg(source);
    let out = cmd.output().expect("spawn elle");
    (
        String::from_utf8_lossy(&out.stdout).into_owned(),
        String::from_utf8_lossy(&out.stderr).into_owned(),
        out.status,
    )
}

#[test]
fn flip_on_by_default_in_lir() {
    let (out, _, status) = run(
        &["--dump=lir"],
        "(defn loop [n] (if (= n 0) :done (loop (- n 1))))",
    );
    assert!(status.success());
    assert!(
        out.contains("flip-enter"),
        "expected flip-enter by default:\n{}",
        out
    );
}

#[test]
fn flip_off_suppresses_instructions() {
    let (out, _, status) = run(
        &["--flip=off", "--dump=lir"],
        "(defn loop [n] (if (= n 0) :done (loop (- n 1))))",
    );
    assert!(status.success());
    assert!(
        !out.contains("flip-enter"),
        "unexpected flip-enter with --flip=off:\n{}",
        out
    );
    assert!(!out.contains("flip-swap"), "unexpected flip-swap:\n{}", out);
    assert!(!out.contains("flip-exit"), "unexpected flip-exit:\n{}", out);
}

#[test]
fn flip_on_injects_at_entry_exit_and_before_tail_call() {
    let (out, _, status) = run(
        &["--flip=on", "--dump=lir"],
        "(defn loop [n] (if (= n 0) :done (loop (- n 1))))",
    );
    assert!(status.success(), "compile failed with --flip=on");
    assert!(
        out.contains("flip-enter"),
        "missing flip-enter with --flip=on:\n{}",
        out
    );
    assert!(
        out.contains("flip-swap"),
        "missing flip-swap (tail call rewrite):\n{}",
        out
    );
    assert!(
        out.contains("flip-exit"),
        "missing flip-exit (before Return):\n{}",
        out
    );
}

#[test]
fn flip_on_runs_a_tail_loop_correctly() {
    // 10k iterations: would blow the slab if rotation is broken, or
    // return the wrong result if the swap pool frees live values.
    let (out, _err, status) = run(
        &["--flip=on", "--jit=0"],
        "(defn loop [n] (if (= n 0) :done (loop (- n 1)))) \
         (println (loop 10000))",
    );
    assert!(
        status.success(),
        "elle exited non-zero with --flip=on:\nstdout: {}\nstderr: {}",
        out,
        _err
    );
    assert!(out.contains("done"), "unexpected output: {}", out);
}

#[test]
fn flip_on_injects_at_while_back_edge() {
    let (out, _, status) = run(
        &["--flip=on", "--dump=lir"],
        "(defn f [] (def @i 0) (while (< i 10) (assign i (+ i 1))))",
    );
    assert!(status.success(), "compile failed with --flip=on");
    let flip_enter_count = out.matches("flip-enter").count();
    assert!(
        flip_enter_count >= 2,
        "expected at least 2 flip-enter (function + while), got {}:\n{}",
        flip_enter_count,
        out
    );
    assert!(
        out.matches("flip-swap").count() >= 1,
        "missing flip-swap for while back-edge:\n{}",
        out
    );
}

#[test]
fn flip_on_while_loop_correct() {
    let (out, err, status) = run(
        &["--flip=on", "--jit=0"],
        "(defn f [] \
           (def @i 0) \
           (def @sum 0) \
           (while (< i 10000) \
             (assign sum (+ sum i)) \
             (assign i (+ i 1))) \
           sum) \
         (println (f))",
    );
    assert!(
        status.success(),
        "elle exited non-zero:\nstdout: {}\nstderr: {}",
        out,
        err
    );
    assert!(out.contains("49995000"), "expected 49995000, got: {}", out);
}

#[test]
fn flip_on_nested_while_correct() {
    let (out, err, status) = run(
        &["--flip=on", "--jit=0"],
        "(defn f [] \
           (def @total 0) \
           (def @i 0) \
           (while (< i 100) \
             (def @j 0) \
             (while (< j 100) \
               (assign total (+ total 1)) \
               (assign j (+ j 1))) \
             (assign i (+ i 1))) \
           total) \
         (println (f))",
    );
    assert!(
        status.success(),
        "elle exited non-zero:\nstdout: {}\nstderr: {}",
        out,
        err
    );
    assert!(out.contains("10000"), "expected 10000, got: {}", out);
}

#[test]
fn flip_on_break_from_while() {
    let (out, err, status) = run(
        &["--flip=on", "--jit=0"],
        "(println (block :x (while true (break :x 42))))",
    );
    assert!(
        status.success(),
        "elle exited non-zero:\nstdout: {}\nstderr: {}",
        out,
        err
    );
    assert!(out.contains("42"), "expected 42, got: {}", out);
}

#[test]
fn flip_on_unsafe_while_no_flip_injected() {
    // A while loop that pushes heap values (pair cells) to an outer binding
    // must NOT get FlipSwap — the outward set makes it unsafe.
    let (out, _, status) = run(
        &["--flip=on", "--dump=lir"],
        "(defn f [] \
           (def @acc nil) \
           (def @i 0) \
           (while (< i 3) \
             (assign acc (pair i acc)) \
             (assign i (+ i 1))) \
           acc)",
    );
    assert!(status.success(), "compile failed with --flip=on");
    // No flip-swap should appear — function-level flip-swap only fires
    // at tail calls, and while-loop flip-swap was rejected by safety analysis.
    let flip_swap_count = out.matches("flip-swap").count();
    assert!(
        flip_swap_count == 0,
        "expected 0 flip-swap (unsafe while should be rejected), got {}:\n{}",
        flip_swap_count,
        out
    );
}

#[test]
fn flip_invalid_value_is_rejected() {
    let (_, err, status) = run(&["--flip=maybe"], "(+ 1 2)");
    assert!(!status.success());
    assert!(
        err.contains("--flip: expected on/off"),
        "expected helpful error, got: {}",
        err
    );
}
