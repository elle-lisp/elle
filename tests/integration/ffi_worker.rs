// audited: 2026-10-22
// A worker that leaves a library's thread-exit destructor armed exits cleanly, because no worker unmaps the library.
//
// docs/ffi.md
//
// A foreign library may call `pthread_key_create(&key, destructor)`, and a
// worker that stores a non-null value under that key has glibc call
// `destructor` from `__nptl_deallocate_tsd` as its OS thread exits. Were the
// library `dlclose`d on worker teardown, that walk would jump into an unmapped
// page and the SIGSEGV would kill the whole process. src/ffi/registry.rs owns
// every mapping for the life of the process and never `dlclose`s one, so the
// destructor always lands in mapped code. Each test is gated on a C compiler,
// and does nothing without one.

use std::os::unix::process::ExitStatusExt;
use std::path::PathBuf;
use std::process::{Command, ExitStatus};

// A pthread TLS key whose destructor lives in THIS `.so`. `reg_tls_dtor` arms it
// (stores a non-null value, so the destructor runs at thread exit); on first call
// it creates the key. `clear_tls_dtor` deletes the key, so the destructor will NOT
// run.
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

/// The fixture library, built in a directory of its own and removed with it.
///
/// The trap: both tests run at once in one process. Were the fixture built at
/// one fixed path, one test's compiler would rewrite the `.so` while the
/// other's worker loads it. `dlopen` would refuse the half-written file as
/// "file too short", and the worker would exit 1 and fail the clean-exit check.
struct Fixture(PathBuf);

impl Fixture {
    /// Compile the fixture under `OUT_DIR`, which sits in the cargo target tree
    /// and so is mapped executable, as `dlopen` requires: a tmpfs such as
    /// `/dev/shm` may not be. `None` when no C compiler is available.
    fn build(tag: &str) -> Option<Fixture> {
        use std::sync::atomic::{AtomicUsize, Ordering};
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        let n = NEXT.fetch_add(1, Ordering::Relaxed);
        let dir = PathBuf::from(env!("OUT_DIR"))
            .join(format!("ffi-worker-{tag}-{}-{n}", std::process::id()));
        std::fs::create_dir_all(&dir).expect("create fixture dir");
        let fixture = Fixture(dir);
        let src = fixture.0.join("tlsdtor.c");
        std::fs::write(&src, FIXTURE_C).expect("write fixture source");
        let cc = std::env::var("CC").unwrap_or_else(|_| "cc".into());
        match Command::new(cc)
            .args(["-shared", "-fPIC", "-O2", "-pthread", "-o"])
            .arg(fixture.so())
            .arg(&src)
            .status()
        {
            Ok(s) if s.success() => Some(fixture),
            _ => None,
        }
    }

    fn so(&self) -> PathBuf {
        self.0.join("libtlsdtor.so")
    }

    fn so_str(&self) -> String {
        self.so()
            .to_str()
            .expect("fixture path is utf-8")
            .to_string()
    }
}

impl Drop for Fixture {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
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
    let Some(fixture) = Fixture::build("teardown") else {
        return; // no C compiler on this host; nothing to exercise
    };
    let so = fixture.so_str();

    // A worker loads the fixture, arms a thread-local destructor whose code
    // lives in the `.so`, and exits WITHOUT deleting the key. A per-worker
    // `dlclose` would unmap the destructor's page before thread exit, and the
    // process would die of SIGSEGV. The clean exit shows the mapping outlives
    // the worker.
    let clean = run_worker(&so, false);
    assert_eq!(
        clean.signal(),
        None,
        "a worker that loaded an FFI lib with a live TLS destructor and exited \
         without manual teardown was killed by signal {:?}. The library mapping is \
         unloaded on worker teardown. Exit code {:?}.",
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
    let cleared = run_worker(&so, true);
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
    let Some(fixture) = Fixture::build("two-workers") else {
        return;
    };
    let so = fixture.so_str();

    // Two workers load the SAME library (dedup by canonical path in the registry)
    // and both exit without teardown. Since the mapping is never unloaded, neither
    // worker's exit unmaps the destructor the other (or it itself) still has armed.
    let status = run_two_workers(&so);
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
