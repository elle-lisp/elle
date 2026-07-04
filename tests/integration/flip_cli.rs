// `--flip=on` CLI surface tests.
//
// The `--flip=on/off` flag is accepted for compatibility but is a no-op:
// the lowerer emits no flip bytecodes (FlipEnter/FlipSwap/FlipExit).
//
// While/loop reclamation uses RegionRotate.
// Self-tail-call reclamation uses mark/release in the trampoline.

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
fn flip_on_no_longer_injects_flip_instructions() {
    // --flip=on is a no-op; the lowerer emits no flip bytecodes.
    let (out, _, status) = run(
        &["--flip=on", "--dump=lir"],
        "(defn f [] (def @i 0) (while (%lt i 10) (assign i (%add i 1))))",
    );
    assert!(status.success(), "compile failed with --flip=on");
    assert!(
        !out.contains("flip-enter"),
        "flip instructions should not be injected:\n{}",
        out
    );
}

#[test]
fn flip_on_runs_a_tail_loop_correctly() {
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
fn flip_invalid_value_is_rejected() {
    let (_, err, status) = run(&["--flip=maybe"], "(+ 1 2)");
    assert!(!status.success());
    assert!(
        err.contains("--flip: expected on/off"),
        "expected helpful error, got: {}",
        err
    );
}
