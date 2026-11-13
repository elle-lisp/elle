// audited: 2026-11-13
// A worker that loads an FFI library and exits with a thread-exit destructor still armed exits cleanly.
//
// docs/ffi.md
//
// The trap: a C library can register a destructor with `pthread_key_create` and
// arm it with `pthread_setspecific`. Glibc then calls that destructor, which is
// code in the library, from `__nptl_deallocate_tsd` as the thread exits. If the
// library were unmapped before a worker's thread exited, that call would jump
// into an unmapped page, and the SIGSEGV would end the whole process.
//
// The counter-factual: a worker that deletes its key before it exits runs no
// destructor, so it exits cleanly whether or not the library stays mapped. Only
// a worker that leaves the destructor armed tells the two apart.
//
// Each test builds its own copy of the fixture, in a directory of its own under
// `OUT_DIR`. The tests run in parallel, and a build that rewrote the library
// another test was loading would fail that test's `ffi/native`. `OUT_DIR` is on
// the cargo target tree, which `dlopen` can map executable, and the platform
// temp root need not be. Without a C compiler, each test returns at once.

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

// A pthread TLS key whose destructor lives in this `.so`. `reg_tls_dtor` creates
// the key on its first call, and arms it by storing a non-null value, so the
// destructor runs at thread exit. `clear_tls_dtor` deletes the key, so the
// destructor does not run.
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

/// Compile the TLS-destructor fixture into a directory of its own under
/// `OUT_DIR`, removed when the answer drops. The answer reads as the `.so`
/// path. Answers `None` when no C compiler is available, and the test then has
/// nothing to exercise.
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

/// Run a worker that loads the fixture, arms the TLS destructor, and, when
/// `teardown` holds, deletes the key before it returns. The worker body is a
/// flat sequence of `def`s and direct `ffi/call`s, with no `defn`, so it
/// exercises the library load and the thread exit alone.
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

/// Run two workers that both load the same fixture library, and both exit
/// without clearing the key. The registry holds one entry for the two loads.
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

    // A worker that leaves the destructor armed exits cleanly only while the
    // library stays mapped past the worker's thread exit.
    let clean = run_worker(so, false);
    assert_eq!(
        clean.signal(),
        None,
        "a worker that loaded an FFI library and exited with its TLS destructor \
         armed was killed by signal {:?}, so the library was unmapped before the \
         worker's thread exited. Exit code {:?}.",
        clean.signal(),
        clean.code(),
    );
    assert_eq!(
        clean.code(),
        Some(0),
        "the worker that leaves its destructor armed exits 0; got {:?}",
        clean.code()
    );

    // A worker that deletes the key before it exits exits cleanly too.
    let cleared = run_worker(so, true);
    assert_eq!(
        cleared.signal(),
        None,
        "a worker that deleted its TLS key was killed by signal {:?}",
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

    // Two workers load one library, which the registry keys by canonical path,
    // and both exit with the destructor armed. Neither exit unmaps the code the
    // other worker's destructor runs.
    let status = run_two_workers(so);
    assert_eq!(
        status.signal(),
        None,
        "two workers that loaded one FFI library were killed by signal {:?}, \
         exit {:?}",
        status.signal(),
        status.code(),
    );
    assert_eq!(status.code(), Some(0));
}
