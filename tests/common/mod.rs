// audited: 2026-09-22
//! Shared test helpers: the canonical evals, the cached ones property tests
//! use, and the scratch directory a test writes files under.
//! It also holds the readers the repository's own tests share: the corpus, the
//! Makefile, and the workflow files.
//!
//! tests/AGENTS.md
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
#[allow(dead_code)]
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
// that need a guaranteed-fresh Runtime.

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
                core.load_stdlib(&elle::compiler::stdlib_cache::StdlibCache::Off);
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

/// One Makefile variable's value, as `make` itself expands it, or `None` if
/// `make` could not be run.
///
/// Tests that check how CI dimensions the corpus — job counts, per-file
/// budgets — have to read the value the pass will use, not the assignment's
/// text. Those two differ: a variable can sit inside an `ifdef`, be computed by
/// `$(shell …)`, be continued across lines with a backslash, or reference
/// another variable. A test that parses the Makefile reimplements `make`, and
/// the first thing such a parser does is disagree with it. This asks `make`.
///
/// `env` sets variables for the child, over a slate cleared of `GITHUB_ACTIONS`
/// and `JOBS`: the answer must not depend on whether the suite itself is
/// running under CI.
///
/// The Makefile's `print-%` rule is what this reads through.
#[allow(dead_code)]
pub fn make_var(name: &str, env: &[(&str, &str)]) -> Option<String> {
    let mut command = std::process::Command::new("make");
    command
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--no-print-directory")
        .arg(format!("print-{name}"))
        .env_remove("GITHUB_ACTIONS")
        .env_remove("JOBS");
    for (key, value) in env {
        command.env(key, value);
    }
    let out = command.output().ok()?;
    if !out.status.success() {
        return None;
    }
    Some(String::from_utf8(out.stdout).ok()?.trim().to_string())
}

/// The commands `make TARGET` will run, expanded, without running any of them.
///
/// `make_var` above answers one variable; a recipe is where those variables
/// meet the flags written beside them, and a test about what a pass actually
/// runs has to read the whole line. `--dry-run` is what prints it: every
/// variable resolved, every `@` line shown, nothing executed.
///
/// The child runs over a slate cleared of `GITHUB_ACTIONS` and `JOBS`, for the
/// reason `make_var` clears them.
#[allow(dead_code)]
pub fn make_dry_run(target: &str) -> Option<String> {
    let out = std::process::Command::new("make")
        .current_dir(env!("CARGO_MANIFEST_DIR"))
        .arg("--dry-run")
        .arg("--no-print-directory")
        .arg(target)
        .env_remove("GITHUB_ACTIONS")
        .env_remove("JOBS")
        .output()
        .ok()?;
    if !out.status.success() {
        return None;
    }
    String::from_utf8(out.stdout).ok()
}

/// Fill `depth + 1` stack frames with `pattern`, so that any construction
/// temporary a later call materializes inherits pattern bytes in its padding.
///
/// A determinism test paints between two dumps: a dumper that copied slot
/// bytes wholesale would write whatever the stack held into the artifact. The
/// xor keeps the recursion and the buffer observable.
#[allow(dead_code)]
#[inline(never)]
pub fn paint_stack(pattern: u8, depth: usize) -> u64 {
    let buf = [pattern; 4096];
    let sum: u64 = buf.iter().map(|&b| b as u64).sum();
    if depth == 0 {
        sum
    } else {
        sum ^ paint_stack(pattern, depth - 1)
    }
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

    /// The directory itself, for a caller that hands it to something taking a
    /// directory rather than a file.
    pub fn path(&self) -> &std::path::Path {
        &self.0
    }
}

impl Drop for ScratchDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

// ---------------------------------------------------------------------------
// The corpus, and what the Makefile says about it
// ---------------------------------------------------------------------------
//
// Two test files ask the same questions of the corpus and the Makefile: which
// files it holds, which of them declare a deadline of their own, and which
// families the Makefile gives a wider budget. `budget.rs` asks about the
// one-process-per-file passes and `runner_budget.rs` about `elle test`. The
// readers live here because a second copy of them is a second answer, and the
// two files exist to check that the two budgets agree.

/// The repository root, as the test binary was compiled against it.
#[allow(dead_code)]
pub fn repo_root() -> std::path::PathBuf {
    std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// The Makefile's text.
#[allow(dead_code)]
pub fn makefile() -> String {
    std::fs::read_to_string(repo_root().join("Makefile")).expect("read the Makefile")
}

/// One Makefile variable, as `make` expands it.
///
/// Asking `make` rather than parsing the assignment is the whole point: these
/// tests measure what a pass will actually run, and a parser that reimplements
/// variable references, line continuations and `$(shell …)` is a second `make`
/// that can disagree with the first.
#[allow(dead_code)]
pub fn make_expand(name: &str) -> String {
    make_var(name, &[]).unwrap_or_else(|| panic!("`make print-{name}` did not run"))
}

/// The path patterns the Makefile gives the wider budget.
///
/// `WIDE_FILES` is a `grep` pattern list, `-e one -e two` — the shape the
/// per-pass skip lists beside it already use. A pattern is a substring of a
/// path, not a file name: a whole family of corpus files shares one deadline
/// and one prefix, so the list names the prefix rather than every member.
#[allow(dead_code)]
pub fn wide_patterns() -> Vec<String> {
    let patterns = make_expand("WIDE_FILES");
    let names: Vec<String> = patterns
        .split_whitespace()
        .filter(|word| *word != "-e")
        .map(str::to_string)
        .collect();
    assert!(
        !names.is_empty(),
        "WIDE_FILES does not read as a `grep` pattern list: {patterns}"
    );
    names
}

/// Every corpus file, as a repo-relative path.
#[allow(dead_code)]
pub fn corpus_files() -> Vec<String> {
    let mut paths: Vec<String> = std::fs::read_dir(repo_root().join("tests/elle"))
        .expect("read tests/elle")
        .map(|entry| entry.expect("a corpus directory entry").file_name())
        .filter_map(|name| name.to_str().map(str::to_string))
        .filter(|name| name.ends_with(".lisp"))
        .map(|name| format!("tests/elle/{name}"))
        .collect();
    paths.sort();
    assert!(paths.len() > 100, "the corpus did not read: {paths:?}");
    paths
}

/// The deadline a corpus file gives itself, if it declares one.
///
/// A file that has to detect a stall carries `(def deadline N)` and reports
/// through it — which request stalled, and how long it waited. That number is
/// in seconds, and it is the only thing that knows what the file considers
/// hung.
#[allow(dead_code)]
pub fn declared_deadline(path: &str) -> Option<u64> {
    let source = std::fs::read_to_string(repo_root().join(path)).expect("read a corpus file");
    let (_, rest) = source.split_once("(def deadline ")?;
    let digits = rest.split(')').next()?.trim();
    Some(
        digits
            .parse()
            .unwrap_or_else(|_| panic!("{path} declares a deadline this cannot read: {digits}")),
    )
}

// ---------------------------------------------------------------------------
// The workflow files, and the jobs in them
// ---------------------------------------------------------------------------
//
// `workflows.rs` asks what the pull-request gate waits for, and
// `run_artifacts.rs` asks what each corpus job leaves behind. Both need the
// same reading — a workflow file split into jobs — and a second copy of it is a
// second answer to "what is a job".

/// Every workflow file, as (repo-relative path, text).
#[allow(dead_code)]
pub fn workflow_files() -> Vec<(String, String)> {
    let dir = repo_root().join(".github/workflows");
    let mut out: Vec<(String, String)> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("read {}: {e}", dir.display()))
        .filter_map(|entry| {
            let path = entry.expect("a workflow directory entry").path();
            let name = path.file_name()?.to_str()?.to_string();
            if !name.ends_with(".yml") && !name.ends_with(".yaml") {
                return None;
            }
            let text = std::fs::read_to_string(&path).expect("read a workflow file");
            Some((format!(".github/workflows/{name}"), text))
        })
        .collect();
    out.sort();
    assert!(
        !out.is_empty(),
        "no workflow files read from {}",
        dir.display()
    );
    out
}

/// `  name:` at exactly two spaces of indent — a key in the `jobs` mapping.
/// A comment at that indent has no bare identifier before the colon, so the
/// character check rejects it.
fn job_header(line: &str) -> Option<String> {
    let rest = line.strip_prefix("  ")?;
    if rest.starts_with(' ') {
        return None;
    }
    let name = rest.strip_suffix(':')?;
    let ident = |c: char| c.is_ascii_alphanumeric() || c == '-' || c == '_';
    if name.is_empty() || !name.chars().all(ident) {
        return None;
    }
    Some(name.to_string())
}

/// Every job in one workflow, as (name, body). Comment lines are dropped from
/// the body: the trap is that several jobs discuss commands they do not run —
/// the WASM job's comment names `make smoke-wasm` while running `check-wasm` —
/// so a body scan that kept comments would read those as steps.
#[allow(dead_code)]
pub fn workflow_jobs(text: &str) -> Vec<(String, String)> {
    let mut out: Vec<(String, String)> = Vec::new();
    let mut current: Option<(String, String)> = None;
    let mut in_jobs = false;

    for line in text.lines() {
        if line == "jobs:" {
            in_jobs = true;
            continue;
        }
        if !in_jobs {
            continue;
        }
        // Any other top-level key closes the `jobs` mapping.
        if !line.starts_with(' ') && !line.trim().is_empty() {
            break;
        }
        if let Some(name) = job_header(line) {
            out.extend(current.take());
            current = Some((name, String::new()));
            continue;
        }
        if let Some((_, body)) = current.as_mut() {
            if !line.trim_start().starts_with('#') {
                body.push_str(line);
                body.push('\n');
            }
        }
    }
    out.extend(current);
    out
}

/// A `timeout` argument as a number: `120s` is 120.
#[allow(dead_code)]
pub fn budget_seconds(budget: &str) -> u64 {
    budget
        .trim()
        .trim_end_matches('s')
        .parse()
        .unwrap_or_else(|_| panic!("a budget is a whole number of seconds: {budget}"))
}
