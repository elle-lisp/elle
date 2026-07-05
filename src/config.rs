//! Global configuration parsed from CLI arguments.
//!
//! Set once at startup via `init`, read anywhere via `get`.
//! Runtime configuration parsed from CLI flags. See `Config::parse` and `elle --help`.

use std::collections::HashSet;
use std::sync::OnceLock;

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Legacy: flip instructions are always no-ops. Kept for API compat.
pub fn flip_enabled() -> bool {
    false
}

/// Default cache directory.
///
/// Resolution order:
/// 1. `ELLE_CACHE` env var (empty string = no caching)
/// 2. `$TMPDIR/elle-cache`
/// 3. `$TMP/elle-cache`
/// 4. No caching
fn default_cache_dir() -> Option<String> {
    if let Ok(v) = std::env::var("ELLE_CACHE") {
        return if v.is_empty() { None } else { Some(v) };
    }
    let base = std::env::var("TMPDIR")
        .or_else(|_| std::env::var("TMP"))
        .ok()?;
    Some(format!("{}/elle-cache", base))
}

/// Read the global config. Returns default if `init` hasn't been called.
pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

/// Initialize the global config. Must be called before `get` for
/// CLI-parsed values to take effect. No-op if already initialized.
pub fn init(config: Config) {
    let _ = CONFIG.set(config);
}

/// Whether `%`-intrinsics route through their checked `NativeFn` primitives
/// (the CLI default, the escape-correct native-Call path) rather than inlining
/// to unchecked opcodes — read by the analyzer's intrinsic-recognition gate.
///
/// This is a function rather than a bare `get().checked_intrinsics` read so a
/// test can exercise the checked-on path without touching the write-once global
/// `CONFIG` (which is process-wide and first-write-wins, so unsafe to flip per
/// test under parallelism). In a non-test build it is exactly the global field.
pub fn checked_intrinsics() -> bool {
    #[cfg(test)]
    {
        if let Some(v) = test_override::get() {
            return v;
        }
    }
    get().checked_intrinsics
}

/// Test-only, thread-local override for [`checked_intrinsics`]. A unit test that
/// must compile on the checked-on (native-Call) path sets it for the duration of
/// one compile and clears it — scoped to the calling thread, so it cannot leak
/// across the test runner's parallel threads. Use [`ScopedCheckedIntrinsics`].
#[cfg(test)]
pub(crate) mod test_override {
    use std::cell::Cell;
    thread_local! {
        static OVERRIDE: Cell<Option<bool>> = const { Cell::new(None) };
    }
    pub(crate) fn get() -> Option<bool> {
        OVERRIDE.with(|c| c.get())
    }
    fn set(v: Option<bool>) {
        OVERRIDE.with(|c| c.set(v));
    }

    /// RAII guard: forces `checked_intrinsics()` to `value` on this thread until
    /// dropped, then restores the prior override (clearing on the common path).
    pub(crate) struct ScopedCheckedIntrinsics(Option<bool>);
    impl ScopedCheckedIntrinsics {
        pub(crate) fn new(value: bool) -> Self {
            let prev = get();
            set(Some(value));
            ScopedCheckedIntrinsics(prev)
        }
    }
    impl Drop for ScopedCheckedIntrinsics {
        fn drop(&mut self) {
            set(self.0);
        }
    }
}

mod policy;
pub use policy::{JitPolicy, MlirPolicy, WasmPolicy};

mod keywords;
pub use keywords::{dump_bits, trace_bits, DUMP_KEYWORDS, TRACE_KEYWORDS};

/// Process-global mirror of the active VM's trace bits, kept in sync by
/// `RuntimeConfig::set_trace` and `from_static_config`. Threadpool
/// worker threads, signal-handler-adjacent code, and other off-VM call
/// sites (which can't carry a `&VM` reference) check this directly via
/// `global_trace_bit_enabled`. In a multi-VM process the most recent
/// `set_trace` wins — adequate for the diagnostic use case.
pub static GLOBAL_TRACE_BITS: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

/// Fast check for off-VM trace gates. Single relaxed atomic load when
/// the bit is off; format args are only evaluated when the bit is on.
#[inline]
pub fn global_trace_bit_enabled(bit: u32) -> bool {
    GLOBAL_TRACE_BITS.load(std::sync::atomic::Ordering::Relaxed) & bit != 0
}

// ── RuntimeConfig ─────────────────────────────────────────────────

/// Mutable runtime configuration stored on the VM.
///
/// Accessible from Elle via `(vm/config)`. Changes take effect immediately.
/// Separate from `Config` (which is static/global) so that per-fiber or
/// per-test configuration is possible.
#[derive(Debug, Clone)]
pub struct RuntimeConfig {
    /// Active trace keywords.
    pub trace: HashSet<String>,
    /// Bitfield cache mirroring `trace` for fast hot-path checks.
    pub trace_bits: u32,
    /// JIT compilation policy.
    pub jit: JitPolicy,
    /// WASM compilation policy.
    pub wasm: WasmPolicy,
    /// MLIR compilation policy for GPU-eligible functions.
    pub mlir: MlirPolicy,
    /// Print bytecode before execution.
    pub debug_bytecode: bool,
    /// Print compilation stats on exit.
    pub stats: bool,
}

impl RuntimeConfig {
    /// Build a RuntimeConfig from the static global Config.
    pub fn from_static_config(config: &Config) -> Self {
        let mut trace = HashSet::new();
        let mut bits = 0u32;
        for kw in &config.trace_keywords {
            trace.insert(kw.clone());
            bits |= trace_bits::from_name(kw);
        }
        // Mirror to the process-global so off-VM call sites can gate
        // tracing without a VM reference. See `GLOBAL_TRACE_BITS`.
        GLOBAL_TRACE_BITS.store(bits, std::sync::atomic::Ordering::Relaxed);

        RuntimeConfig {
            trace,
            trace_bits: bits,
            jit: config.jit.clone(),
            wasm: config.wasm.clone(),
            mlir: config.mlir.clone(),
            debug_bytecode: bits & trace_bits::BYTECODE != 0,
            stats: config.stats,
        }
    }

    /// Set the trace keyword set and update the bitfield cache.
    pub fn set_trace(&mut self, keywords: HashSet<String>) {
        let mut bits = 0u32;
        for kw in &keywords {
            bits |= trace_bits::from_name(kw);
        }
        self.trace = keywords;
        self.trace_bits = bits;
        // Keep the off-VM mirror in sync.
        GLOBAL_TRACE_BITS.store(bits, std::sync::atomic::Ordering::Relaxed);
    }

    /// Check if a trace bit is set (fast path — no HashSet lookup).
    #[inline(always)]
    pub fn has_trace_bit(&self, bit: u32) -> bool {
        self.trace_bits & bit != 0
    }
}

impl Default for RuntimeConfig {
    fn default() -> Self {
        RuntimeConfig {
            trace: HashSet::new(),
            trace_bits: 0,
            jit: JitPolicy::Adaptive { threshold: 10 },
            wasm: WasmPolicy::Off,
            mlir: MlirPolicy::Adaptive { threshold: 10 },
            debug_bytecode: false,
            stats: false,
        }
    }
}

// ── Config (static) ───────────────────────────────────────────────

/// All runtime configuration for Elle.
///
/// ## `--jit=N`
///
/// Controls JIT compilation threshold:
/// - `0` — JIT disabled
/// - `N` — JIT enabled, compile after N-1 calls
///   (so `--jit=1` compiles on first call, `--jit=11` compiles after 10)
///
/// Default: 11 (threshold 10).
///
/// ## `--wasm=N`
///
/// Controls WASM tiered compilation:
/// - `0` or omitted — WASM disabled
/// - `N` — tiered WASM enabled, compile after N-1 calls
/// - `full` — full-module WASM backend (compile everything upfront)
///
/// Default: 0 (disabled).
#[derive(Debug, Clone)]
pub struct Config {
    /// JIT compilation policy.
    pub jit: JitPolicy,

    /// Print compilation stats on exit.
    pub stats: bool,

    /// MLIR compilation policy for GPU-eligible functions.
    pub mlir: MlirPolicy,

    /// WASM compilation policy.
    pub wasm: WasmPolicy,

    /// Skip stdlib loading entirely (primitives only).
    pub no_stdlib: bool,

    /// Disk cache directory (WASM compilation, future uses).
    /// `None` = caching disabled (explicit `--cache=""`).
    /// `Some(path)` = cache at that path.
    pub cache: Option<String>,

    // -- I/O --
    /// Disable io_uring on Linux.
    pub no_uring: bool,

    // -- Paths --
    /// Elle home directory (module resolution root).
    pub home: Option<String>,

    /// Colon-separated module search path.
    pub path: Option<String>,

    // -- Output --
    /// JSON output on stderr (errors, stats, timing).
    pub json: bool,

    /// Dump WASM module bytes to /dev/shm/elle-wasm-dump.wasm.
    pub wasm_dump: bool,

    /// Print LIR before WASM emission.
    pub wasm_lir: bool,

    /// Chunk user expressions into sub-thunks (experimental).
    pub wasm_chunk: bool,

    /// Sparse spill: only spill live registers at suspend points.
    /// Reduces code size from O(total_regs * suspend_points) to
    /// O(live_regs * suspend_points). On by default.
    pub wasm_sparse_spill: bool,

    /// Route %-intrinsic calls through registered NativeFn primitives
    /// with runtime type validation instead of inlining to unchecked
    /// BinOp/CmpOp/etc. Implies jit=off, mlir=off.
    ///
    /// CLI default: **on** (set by `Config::parse`; the struct `Default` is
    /// off, the library/test baseline). Only the native `Call` path is
    /// escape-correct — the inlined `%`-intrinsic opcodes (and the JIT's
    /// inlined accessors) are not. Routing `%`-ops as real native `Call`s is
    /// therefore the sound default; the opcode fast-path is an optimization to
    /// be re-enabled later. Override with
    /// `--checked-intrinsics=off` (restores the optimizing tiers) or by
    /// explicitly enabling `--jit`/`--mlir`.
    pub checked_intrinsics: bool,

    /// Enable the A-normal form lift pass (`src/hir/anf.rs`).
    ///
    /// Default: on. `--anf=off` short-circuits `anf_lift` to a
    /// no-op — i.e. the HIR is handed to region inference exactly as
    /// `functionalize` produced it, with allocating call results
    /// unnamed and the lowerer falling back on the shadow
    /// `call_region_slot` mechanism (`src/lir/lower/mod.rs`).
    ///
    /// Provided as a counter-factual switch. With `--anf=off` the
    /// closure-binding-overwrite bug (Family C in
    /// `tests/integration/anf_counterfactual.rs`) returns; without
    /// the flag the same scripts pass. This is the canonical proof
    /// that the ANF transform is what closes the bug class — not some
    /// other change between the failing and passing trees.
    ///
    /// Should be removed in a follow-up once causality is reviewed.
    pub anf: bool,

    /// Compiler stages to dump (from `--dump=kw1,kw2,...`). Valid keywords
    /// are listed in `DUMP_KEYWORDS`. When non-empty, the compiler runs up
    /// to each requested stage, prints its artifact, and exits without
    /// executing.
    pub dump: HashSet<String>,

    /// Trace keywords from `--trace=kw1,kw2,...`.
    /// Stored here from CLI parsing, then merged into RuntimeConfig on VM init.
    pub trace_keywords: Vec<String>,

    /// Initial page size for region allocation (CLI: --region-page-size).
    /// Must be a power of two >= 4096. Default: 4096.
    pub region_page_size: usize,

    /// Maximum bytes to cache in the per-thread page pool
    /// (CLI: --page-pool-max). Default: 4MB.
    pub page_pool_max: usize,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            // NOTE: the struct `Default` is the *library/test* baseline (the
            // unoptimized opcode path: checked off, optimizing tiers on). The
            // **CLI** effective default is different — `Config::parse` turns
            // checked_intrinsics ON (and the optimizing tiers off) because that
            // native-Call path is the only escape-correct one. Keeping the
            // struct default at the old values means the region/anf solver unit
            // tests (which exercise the intrinsic-opcode path and run off
            // `Config::default`, never `parse`) stay meaningful. See `Config::parse`.
            jit: JitPolicy::Adaptive { threshold: 10 },
            stats: false,
            mlir: MlirPolicy::Adaptive { threshold: 10 },
            wasm: WasmPolicy::Off,
            no_stdlib: false,
            cache: default_cache_dir(),
            no_uring: false,
            home: std::env::var("ELLE_HOME").ok(),
            path: std::env::var("ELLE_PATH").ok(),
            json: false,
            wasm_dump: false,
            wasm_lir: false,
            wasm_chunk: false,
            wasm_sparse_spill: true,
            checked_intrinsics: false,
            anf: true,
            dump: HashSet::new(),
            trace_keywords: Vec::new(),
            region_page_size: 4096,
            page_pool_max: 4 * 1024 * 1024,
        }
    }
}

mod parse;

impl Config {
    /// Check if a trace keyword is set.
    pub fn has_trace(&self, keyword: &str) -> bool {
        self.trace_keywords.iter().any(|k| k == keyword)
    }

    /// Compute trace bits for this config (bitfield of enabled trace keywords).
    pub fn trace_bits(&self) -> u32 {
        let mut bits = 0u32;
        for kw in &self.trace_keywords {
            bits |= trace_bits::from_name(kw);
        }
        bits
    }

    /// Whether JIT compilation is enabled.
    pub fn jit_enabled(&self) -> bool {
        self.jit.enabled()
    }

    /// Whether WASM tiered compilation is enabled (lazy mode).
    pub fn wasm_tier_enabled(&self) -> bool {
        matches!(self.wasm, WasmPolicy::Lazy { .. })
    }

    /// Whether full-module WASM mode is enabled.
    pub fn wasm_full(&self) -> bool {
        matches!(self.wasm, WasmPolicy::Full)
    }
}

#[cfg(test)]
mod tests;
