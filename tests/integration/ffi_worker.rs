// FFI library lifetime across worker threads: a library that registers thread-local
// destructors must not be unmapped while a thread still has a pending destructor.
//
// The hazard: a foreign library calls `pthread_key_create(&key, destructor)` and a
// worker stores a non-null value under that key, so glibc invokes `destructor` from
// `__nptl_deallocate_tsd` as the OS thread exits. The test runner runs FFI-using
// Elle inside `os/spawn` workers. The OLD behavior dlclose'd the library on worker
// teardown — unmapping its code — so the thread-exit destructor walk jumped into the
// now-unmapped page → a SIGSEGV that killed the whole process, and the only remedy
// was for the programmer to manually delete the key before the worker exited.
//
// The fix (src/ffi/registry.rs): FFI library mappings are owned process-globally and
// NEVER `dlclose`d (the same discipline plugins use). The `dlopen` refcount never
// reaches 0 on a worker, so the mapping stays put and a worker's thread-exit
// destructor always lands in mapped code — no manual teardown required. This file
// pins that: a worker that loads the fixture, arms its TLS destructor, and exits
// WITHOUT clearing the key now exits CLEANLY (the assertion that flipped from the old
// "killed by a signal" is itself the proof the mapping outlives the worker). Gated on
// a C compiler being present; a no-op without one.

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

// A pthread TLS key whose destructor lives in THIS `.so`. `reg_tls_dtor` arms it
// (stores a non-null value, so the destructor runs at thread exit); on first call
// it creates the key. `clear_tls_dtor` deletes the key, so the destructor will NOT
// run. With the OLD per-worker `dlclose`, an armed-but-not-cleared key whose `.so`
// was unmapped left glibc's thread-exit walk pointing into unmapped code — the crash
// this test guarded. With the fix, the `.so` is never unmapped, so the armed
// destructor is always safe to run.
const FIXTURE_C: &str = r#"
#include <pthread.h>
static pthread_key_t key;
static int armed = 0;
static void destructor(void *unused) { (void)unused; }
void reg_tls_dtor(void) {
    if (!armed) { pthread_key_create(&key, destructor); armed = 1; }
    pthread_setspecific(key, (void *)1);
}
void clear_tls_dtor(void) {
    if (armed) { pthread_key_delete(key); armed = 0; }
}
"#;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

/// Compile the TLS-destructor fixture into `OUT_DIR` (under the cargo target tree,
/// which — unlike `/dev/shm` — is mapped executable, a prerequisite for `dlopen`).
/// Returns the `.so` path, or `None` when no C compiler is available, in which case
/// the test no-ops (there is nothing to exercise).
fn build_fixture() -> Option<PathBuf> {
    let out = PathBuf::from(env!("OUT_DIR"));
    let src = out.join("tlsdtor.c");
    let so = out.join("libtlsdtor.so");
    std::fs::write(&src, FIXTURE_C).expect("write fixture source");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    match Command::new(cc)
        .args(["-shared", "-fPIC", "-O2", "-pthread", "-o"])
        .arg(&so)
        .arg(&src)
        .status()
    {
        Ok(s) if s.success() => Some(so),
        _ => None,
    }
}

/// Run a worker that loads the fixture, arms the TLS destructor, and — when
/// `teardown` — deletes the key before returning. The worker body is a flat sequence
/// of `def`s and direct `ffi/call`s (no `defn`), so it exercises only the
/// FFI/teardown path under test.
fn run_worker(so: &str, teardown: bool) -> ExitStatus {
    let clear = if teardown {
        r#"(ffi/call (ffi/lookup lib "clear_tls_dtor") sig)"#
    } else {
        ""
    };
    let snippet = format!(
        r#"(sys/join (sys/spawn (fn []
            (def lib (ffi/native "{so}"))
            (def sig (ffi/signature :void @[]))
            (ffi/call (ffi/lookup lib "reg_tls_dtor") sig)
            {clear}
            0)))"#
    );
    Command::new(elle_binary())
        .arg("-e")
        .arg(snippet)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e")
        .status
}

/// Run two workers that both load the SAME fixture library and both exit without
/// clearing the key — the dedup + no-double-unmap case.
fn run_two_workers(so: &str) -> ExitStatus {
    let body = format!(
        r#"(fn []
            (def lib (ffi/native "{so}"))
            (def sig (ffi/signature :void @[]))
            (ffi/call (ffi/lookup lib "reg_tls_dtor") sig)
            0)"#
    );
    let snippet = format!(
        r#"(def w1 (sys/spawn {body}))
           (def w2 (sys/spawn {body}))
           (sys/join w1)
           (sys/join w2)"#
    );
    Command::new(elle_binary())
        .arg("-e")
        .arg(snippet)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e")
        .status
}

#[test]
fn worker_ffi_load_without_teardown_exits_cleanly() {
    let Some(so) = build_fixture() else {
        return; // no C compiler on this host; nothing to exercise
    };
    let so = so.to_str().expect("fixture path is utf-8");

    // The fix, and the counterfactual that flipped. A worker loads the fixture, arms
    // a thread-local destructor whose code lives in the `.so`, and exits WITHOUT
    // deleting the key. Under the old per-worker `dlclose` this SIGSEGV'd (the
    // destructor's page was unmapped before thread exit); now the mapping is
    // process-global and never unmapped, so the worker exits cleanly. This clean
    // exit is the proof the mapping outlives the worker.
    let clean = run_worker(so, false);
    assert_eq!(
        clean.signal(),
        None,
        "a worker that loaded an FFI lib with a live TLS destructor and exited \
         without manual teardown was killed by signal {:?} — the library mapping is \
         being unloaded on worker teardown (the dlclose-after-TLS-dtor crash this \
         fix removes). Exit code {:?}.",
        clean.signal(),
        clean.code(),
    );
    assert_eq!(
        clean.code(),
        Some(0),
        "the no-teardown worker should exit 0; got {:?}",
        clean.code()
    );

    // Explicitly clearing the key before exit is still valid and clean.
    let cleared = run_worker(so, true);
    assert_eq!(
        cleared.signal(),
        None,
        "clearing the TLS key then exiting should be clean; killed by {:?}",
        cleared.signal()
    );
    assert_eq!(cleared.code(), Some(0));
}

#[test]
fn two_workers_same_lib_no_double_unmap() {
    let Some(so) = build_fixture() else {
        return;
    };
    let so = so.to_str().expect("fixture path is utf-8");

    // Two workers load the SAME library (dedup by canonical path in the registry)
    // and both exit without teardown. Since the mapping is never unloaded, neither
    // worker's exit unmaps the destructor the other (or it itself) still has armed.
    let status = run_two_workers(so);
    assert_eq!(
        status.signal(),
        None,
        "two workers loading the same FFI lib and exiting must not crash; killed by \
         {:?}, exit {:?}",
        status.signal(),
        status.code(),
    );
    assert_eq!(status.code(), Some(0));
}
