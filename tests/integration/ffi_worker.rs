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

use std::ops::Deref;
use std::os::unix::fs::MetadataExt;
use std::os::unix::process::ExitStatusExt;
use std::path::{Path, PathBuf};
use std::process::{Command, Output};

use crate::common::ScratchDir;

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
fn build_fixture() -> Option<Fixture> {
    let dir = ScratchDir::under(Path::new(env!("OUT_DIR")), "ffi-worker");
    let src = dir.join("tlsdtor.c");
    let so = dir.join("libtlsdtor.so");
    std::fs::write(&src, FIXTURE_C).expect("write fixture source");
    let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
    match Command::new(cc)
        .args(["-shared", "-fPIC", "-O2", "-pthread", "-o"])
        .arg(&so)
        .arg(&src)
        .status()
    {
        Ok(s) if s.success() => Some(Fixture { _dir: dir, so }),
        _ => None,
    }
}

/// One build of the fixture: the `.so` path, and the directory that holds it.
struct Fixture {
    _dir: ScratchDir,
    so: PathBuf,
}

impl Deref for Fixture {
    type Target = Path;

    fn deref(&self) -> &Path {
        &self.so
    }
}

/// Run a worker that loads the fixture, arms the TLS destructor, and, when
/// `teardown` holds, deletes the key before it returns. The worker body is a
/// flat sequence of `def`s and direct `ffi/call`s, with no `defn`, so it
/// exercises the library load and the thread exit alone.
fn run_worker(so: &str, teardown: bool) -> Output {
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
    run_elle(&snippet)
}

/// Run two workers that both load the same fixture library, and both exit
/// without clearing the key. The registry holds one entry for the two loads.
fn run_two_workers(so: &str) -> Output {
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
    run_elle(&snippet)
}

fn run_elle(snippet: &str) -> Output {
    Command::new(elle_binary())
        .arg("-e")
        .arg(snippet)
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e")
}

/// Assert that the run `out` of `what` exited 0, killed by no signal. A worker
/// whose `ffi/native` fails exits 1, and only its stderr says why, so the
/// message carries it.
fn assert_clean(out: &Output, what: &str) {
    assert_eq!(
        (out.status.signal(), out.status.code()),
        (None, Some(0)),
        "{what} ended with signal {:?} and exit code {:?}. A signal says the \
         library was unmapped before the worker's thread exited.\nstderr:\n{}",
        out.status.signal(),
        out.status.code(),
        String::from_utf8_lossy(&out.stderr),
    );
}

#[test]
fn worker_ffi_load_without_teardown_exits_cleanly() {
    let Some(so) = build_fixture() else {
        return; // no C compiler on this host; nothing to exercise
    };
    let so = so.to_str().expect("fixture path is utf-8");

    // A worker that leaves the destructor armed exits cleanly only while the
    // library stays mapped past the worker's thread exit.
    assert_clean(
        &run_worker(so, false),
        "a worker that exited with its TLS destructor armed",
    );

    // A worker that deletes the key before it exits exits cleanly too.
    assert_clean(&run_worker(so, true), "a worker that deleted its TLS key");
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
    assert_clean(
        &run_two_workers(so),
        "two workers that loaded one FFI library",
    );
}

#[test]
fn a_fixture_build_leaves_the_library_another_build_answered_in_place() {
    let Some(first) = build_fixture() else {
        return;
    };
    let before = std::fs::metadata(&*first).expect("stat the first fixture");
    let Some(_second) = build_fixture() else {
        return;
    };
    let after = std::fs::metadata(&*first).expect("stat the first fixture again");

    // The counter-factual: two builds that share one path pass the two tests
    // above whenever those tests run apart, and fail one of them with exit
    // code 1 whenever a build truncates the library a worker is loading.
    assert_eq!(
        (before.ino(), before.modified().ok()),
        (after.ino(), after.modified().ok()),
        "a second fixture build rewrote the library the first build answered, at {}",
        first.display()
    );
}
