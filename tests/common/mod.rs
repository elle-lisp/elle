//! Shared test helpers for the Elle test suite.
//!
//! Provides canonical eval and setup functions so test files don't need
//! to copy-paste their own variants.
//!
//! Every helper drives a [`Runtime`] (`elle::runtime`), the one per-instance
//! owner of the heap, `VM`, `SymbolTable`, and per-instance `CompileCtx`. There
//! is no shared compile cache: the compile state each eval names explicitly is
//! the instance's own (`rt.parts()`), so two test instances never share stdlib
//! exports or REPL definitions. `Runtime` also points the VM at its own symbol
//! table and `CompileCtx`, so executed code that resolves through the VM sees
//! this instance's state.

use elle::runtime::{Runtime, RuntimeCore};
use elle::{compile_file, eval_all, Value};

// ── Result inspection must outlive nothing ───────────────────────────────────
//
// A result `Value` is a tagged pointer straight into its `Runtime`'s region
// heap; `Display`, `with_string`, `as_pair`, … deref that pointer directly (no
// ambient heap, no handle). The `Runtime` is torn down — and its `Box<FiberHeap>`
// freed — the instant it drops, so a heap-valued result handed *out* of the eval
// dangles (a use-after-free the plain VM reads as stale-but-intact, only
// guardfree reddens). Immediates carry no pointer and were always safe, which is
// why this stayed latent.
//
// So these helpers are **scoped**: they hand the `Result<Value, String>` to a
// closure that runs while the `Runtime` is still alive, then tear down after.
// Inspect inside `f` and return only OWNED data (scalars, `String`, booleans,
// counts) — never the result `Value` itself, which would re-dangle.
//
// (The cached `eval_reuse*` helpers below do NOT need this: their `RuntimeCore`
// is never torn down, so the heap outlives the returned `Value` for the thread's
// life.)

/// Evaluate Elle source WITHOUT stdlib and inspect the result while its heap is
/// alive (see the module note above). Skips stdlib loading; prelude macros
/// (`defn`, `let*`, `->`, `when`, `try`/`catch`, …) are still available — they
/// live in the `CompileCtx`'s expander, not in the stdlib.
#[allow(dead_code)]
pub fn eval_source_bare<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    let mut rt = Runtime::without_stdlib();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(input, symbols, vm, cctx, "<test>")
    };
    f(result)
}

/// Evaluate Elle source through the full pipeline and inspect the result while
/// its heap is alive (see the module note above). The canonical test eval — use
/// it unless you have a specific reason not to (e.g. testing without stdlib).
/// Handles single- and multi-form input via `eval_all`.
pub fn eval_source<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    let mut rt = Runtime::new();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(input, symbols, vm, cctx, "<test>")
    };
    f(result)
}

/// Like `eval_source` (stdlib loaded) but runs WITHOUT the async scheduler —
/// a plain `vm.execute`, no `ev/run` wrapping. Real top-level Elle always runs
/// scheduled (so `eval_source` does too); use this only for the rare test that
/// asserts behavior *outside* a scheduler — e.g. that an I/O primitive's
/// SIG_IO yield errors at top level when nothing is there to service it.
#[allow(dead_code)]
pub fn eval_source_unscheduled<R>(input: &str, f: impl FnOnce(Result<Value, String>) -> R) -> R {
    let mut rt = Runtime::new();
    let result = {
        let (vm, symbols, cctx) = rt.parts();
        compile_file(input, symbols, cctx, "<test>")
            .map_err(|e| e.to_string())
            .and_then(|r| vm.execute(&r.bytecode).map_err(|e| e.to_string()))
    };
    f(result)
}

#[allow(dead_code)]
/// Set up a `Runtime` (primitives + stdlib, contexts installed). Hand out the
/// disjoint `(vm, symbols, cctx)` borrows via `rt.parts()`.
pub fn setup() -> Runtime {
    Runtime::new()
}

/// Create a proptest config that respects the PROPTEST_CASES env var.
///
/// When PROPTEST_CASES is set, its value overrides the given default.
/// This lets CI and local development control case counts uniformly:
///
///   PROPTEST_CASES=8 cargo test    # fast smoke
///   cargo test                     # use per-test defaults
///
/// Regression files are persisted to `tests/proptest-regressions/`.
#[allow(dead_code)]
pub fn proptest_cases(default: u32) -> proptest::prelude::ProptestConfig {
    use proptest::test_runner::FileFailurePersistence;

    let cases = std::env::var("PROPTEST_CASES")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(default);

    proptest::prelude::ProptestConfig {
        cases,
        max_shrink_iters: 128,
        failure_persistence: Some(Box::new(FileFailurePersistence::Direct(
            "tests/proptest-regressions",
        ))),
        ..proptest::prelude::ProptestConfig::default()
    }
}

// ---------------------------------------------------------------------------
// Cached eval helpers for property tests
// ---------------------------------------------------------------------------
//
// These reuse a thread-local `RuntimeCore` across proptest cases, eliminating
// per-case bootstrap cost (VM creation, primitive registration, stdlib
// loading, CompileCtx construction). Between cases the fiber is reset.
//
// `RuntimeCore` (not `Runtime`) is cached deliberately: a `Runtime` runs a
// teardown sweep on `Drop`, and a thread-local that drops at thread exit would
// run that sweep at an unpredictable point, against an instance other cases may
// still be using. `RuntimeCore` has no such `Drop`, so caching it is safe.
//
// Use `eval_reuse_bare` for tests that don't need stdlib (the common case).
// Use `eval_reuse` for tests that need stdlib functions (map, filter, etc.).
//
// The one-shot `eval_source` / `eval_source_bare` remain available for tests
// that need a guaranteed-fresh Runtime (none currently do, but the option
// exists).

use std::cell::RefCell;
use std::thread::LocalKey;

thread_local! {
    static BARE_CACHE: RefCell<Option<RuntimeCore>> = const { RefCell::new(None) };
    static FULL_CACHE: RefCell<Option<RuntimeCore>> = const { RefCell::new(None) };
}

fn eval_with_cache(
    input: &str,
    cache: &'static LocalKey<RefCell<Option<RuntimeCore>>>,
    with_stdlib: bool,
) -> Result<Value, String> {
    cache.with(|cell| {
        let mut borrow = cell.borrow_mut();
        let core = borrow.get_or_insert_with(|| {
            let mut core = RuntimeCore::bare();
            if with_stdlib {
                // `RuntimeCore::bare` already points the VM at this instance's own
                // symbol table, so stdlib-load gensym (and all name resolution)
                // resolves through it.
                core.load_stdlib();
            }
            core
        });

        let (vm, symbols, cctx) = core.parts();

        // Reset per-case state.
        vm.reset_fiber();
        #[cfg(feature = "jit")]
        vm.jit_cache.clear();

        eval_all(input, symbols, vm, cctx, "<test>")
    })
}

/// Evaluate Elle source with a cached Runtime (primitives only, no stdlib).
///
/// Drop-in replacement for `eval_source_bare` in property tests. The Runtime
/// is created once per thread and reused across proptest cases. Between
/// cases, the fiber is reset.
#[allow(dead_code)]
pub fn eval_reuse_bare(input: &str) -> Result<Value, String> {
    eval_with_cache(input, &BARE_CACHE, false)
}

/// Evaluate Elle source with a cached Runtime (primitives + stdlib).
///
/// Drop-in replacement for `eval_source` in property tests. The Runtime
/// is created once per thread and reused across proptest cases. Between
/// cases, the fiber is reset.
#[allow(dead_code)]
pub fn eval_reuse(input: &str) -> Result<Value, String> {
    eval_with_cache(input, &FULL_CACHE, true)
}

/// Uniquely-named scratch directory under the platform temp root, removed
/// recursively on drop — the panic path included, so a failing test leaves no
/// litter in `$TMPDIR`. See `tests/AGENTS.md` § Scratch files.
pub struct ScratchDir(std::path::PathBuf);

#[allow(dead_code)]
impl ScratchDir {
    pub fn new(tag: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("elle-{}-{}", tag, std::process::id()));
        std::fs::create_dir_all(&dir).expect("create scratch dir");
        ScratchDir(dir)
    }

    pub fn join(&self, name: &str) -> std::path::PathBuf {
        self.0.join(name)
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}
