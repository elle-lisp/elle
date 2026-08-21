// A subprocess spawned from a worker thread must NOT inherit the worker's
// signal mask. Elle masks ALL signals on every thread it spawns internally
// (the `os/spawn` worker the `elle test` runner runs whole-file thunks in,
// among others) so its signalfd machinery owns delivery. `fork(2)` copies the
// spawning thread's mask into the child and `execve(2)` preserves it, so a
// child spawned from such a worker would start with every maskable signal
// blocked — `subprocess/kill … 15` (SIGTERM) would queue but never land, and
// `subprocess/wait` would hang forever. Only the unblockable SIGKILL would
// reap it.
//
// `subprocess/exec` installs a `pre_exec` hook that resets the child's mask to
// empty (and restores SIGPIPE to SIG_DFL). This test reproduces the runner's
// masked-worker context directly — it spawns the child inside
// `(os/spawn (fn [] (ev/run …)))`, where the worker has masked everything — and
// reads the child's OWN blocked mask from /proc/self/status.
//
// Counter-factual: before the reset, the worker-spawned child's SigBlk is the
// full maskable set (a long hex string of mostly-f's, definitely not all
// zeros), so the assertion fails. The fix makes it `0000000000000000`.
//
// Linux-specific: it reads /proc and the worker's "mask everything" discipline
// is the Linux signalfd path (gated at the `mod` declaration in mod.rs).

use std::io::Write;
use std::os::unix::process::ExitStatusExt;
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn worker_spawned_child_has_empty_signal_mask() {
    let dir = crate::common::ScratchDir::new("subprocess-sigmask");
    let fixture = dir.join("worker-child-mask.lisp");

    // The spawn happens INSIDE an os/spawn worker (which masks all signals) and
    // its own ev/run, exactly as the test runner executes a whole-file thunk.
    // The worker returns the child's SigBlk line; the main thread asserts it is
    // the empty mask. If the worker's mask leaked, SigBlk would be non-zero.
    let mut f = std::fs::File::create(&fixture).expect("create fixture");
    write!(
        f,
        r#"
(let [out (os/join (os/spawn (fn []
            (ev/run (fn []
              (let [proc (subprocess/exec "sh" ["-c" "grep SigBlk /proc/self/status"])
                    s (string (port/read-all (get proc :stdout)))]
                (subprocess/wait proc)
                s))))))]
  (println out)
  (assert (string/contains? out "0000000000000000")
          (string "worker-spawned child inherited a blocked mask: " (string/trim out)))
  (println "OK"))
"#
    )
    .unwrap();

    let output = Command::new(elle_binary())
        .arg(&fixture)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle");

    let stdout = String::from_utf8_lossy(&output.stdout);
    let stderr = String::from_utf8_lossy(&output.stderr);
    assert_eq!(
        output.status.signal(),
        None,
        "elle was killed by signal {:?}; stdout:\n{}\nstderr:\n{}",
        output.status.signal(),
        stdout,
        stderr
    );
    assert!(
        output.status.success(),
        "worker-spawned child did not get a reset signal mask.\nstdout:\n{}\nstderr:\n{}",
        stdout,
        stderr
    );
}
