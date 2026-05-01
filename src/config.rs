//! Global configuration parsed from CLI arguments.
//!
//! Set once at startup via `init`, read anywhere via `get`.
//! Runtime configuration parsed from CLI flags. See `Config::parse` and `elle --help`.

use std::collections::HashSet;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Separate atomic for runtime-togglable flip; initialized from Config default,
/// updated by `set_flip`, read by `flip_enabled`.
static FLIP_OVERRIDE: AtomicBool = AtomicBool::new(true);

/// Check whether flip instructions are enabled (runtime-togglable).
pub fn flip_enabled() -> bool {
    FLIP_OVERRIDE.load(Ordering::Relaxed)
}

/// Toggle flip instructions at runtime (from vm/config-set :flip).
pub fn set_flip(on: bool) {
    FLIP_OVERRIDE.store(on, Ordering::Relaxed);
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
    FLIP_OVERRIDE.store(config.flip_instructions, Ordering::Relaxed);
    let _ = CONFIG.set(config);
}

// ── JIT policy ────────────────────────────────────────────────────

/// JIT compilation policy.
#[derive(Debug, Clone, PartialEq)]
pub enum JitPolicy {
    /// JIT disabled.
    Off,
    /// Compile on first call.
    Eager,
    /// Compile after N calls (default: threshold=10).
    Adaptive { threshold: usize },
    /// Only compile silent, capture-free functions.
    Conservative,
    /// Defer to an Elle closure stored on the VM (see `vm/config`).
    Custom,
}

impl JitPolicy {
    /// Whether JIT is enabled at all.
    pub fn enabled(&self) -> bool {
        !matches!(self, JitPolicy::Off)
    }

    /// Hotness threshold (calls before compilation).
    /// Returns 0 for Eager, the threshold for Adaptive, usize::MAX for Off.
    pub fn threshold(&self) -> usize {
        match self {
            JitPolicy::Off => usize::MAX,
            JitPolicy::Eager => 0,
            JitPolicy::Adaptive { threshold } => *threshold,
            JitPolicy::Conservative => 10,
            JitPolicy::Custom => 0,
        }
    }

    /// Keyword representation for Elle.
    pub fn keyword(&self) -> &'static str {
        match self {
            JitPolicy::Off => "off",
            JitPolicy::Eager => "eager",
            JitPolicy::Adaptive { .. } => "adaptive",
            JitPolicy::Conservative => "conservative",
            JitPolicy::Custom => "custom",
        }
    }

    /// Parse from a keyword string.
    pub fn from_keyword(s: &str) -> Option<JitPolicy> {
        match s {
            "off" => Some(JitPolicy::Off),
            "eager" => Some(JitPolicy::Eager),
            "adaptive" => Some(JitPolicy::Adaptive { threshold: 10 }),
            "conservative" => Some(JitPolicy::Conservative),
            "custom" => Some(JitPolicy::Custom),
            _ => None,
        }
    }
}

// ── WASM policy ───────────────────────────────────────────────────

/// WASM compilation policy.
#[derive(Debug, Clone, PartialEq)]
pub enum WasmPolicy {
    /// WASM disabled.
    Off,
    /// Compile entire module upfront.
    Full,
    /// Per-function lazy compilation after N calls.
    Lazy { threshold: usize },
    /// Per-module WASM compilation (future).
    Modular,
}

impl WasmPolicy {
    pub fn keyword(&self) -> &'static str {
        match self {
            WasmPolicy::Off => "off",
            WasmPolicy::Full => "full",
            WasmPolicy::Lazy { .. } => "lazy",
            WasmPolicy::Modular => "modular",
        }
    }

    pub fn from_keyword(s: &str) -> Option<WasmPolicy> {
        match s {
            "off" => Some(WasmPolicy::Off),
            "full" => Some(WasmPolicy::Full),
            "lazy" => Some(WasmPolicy::Lazy { threshold: 10 }),
            "modular" => Some(WasmPolicy::Modular),
            _ => None,
        }
    }
}

// ── MLIR policy ──────────────────────────────────────────────────

/// MLIR compilation policy for GPU-eligible functions.
///
/// Independent of the JIT policy. When the `mlir` feature is compiled in,
/// GPU-eligible functions are compiled through MLIR → LLVM. This policy
/// controls when that compilation happens. Functions not eligible for
/// MLIR fall through to the Cranelift JIT regardless.
#[derive(Debug, Clone, PartialEq)]
pub enum MlirPolicy {
    /// MLIR disabled — GPU-eligible functions fall through to JIT.
    Off,
    /// Compile on first eligible call.
    Eager,
    /// Compile after N calls (default: threshold=10).
    Adaptive { threshold: usize },
}

impl MlirPolicy {
    /// Whether MLIR compilation is enabled at all.
    pub fn enabled(&self) -> bool {
        !matches!(self, MlirPolicy::Off)
    }

    /// Hotness threshold (calls before compilation).
    /// Returns 0 for Eager, the threshold for Adaptive, usize::MAX for Off.
    pub fn threshold(&self) -> usize {
        match self {
            MlirPolicy::Off => usize::MAX,
            MlirPolicy::Eager => 0,
            MlirPolicy::Adaptive { threshold } => *threshold,
        }
    }

    /// Keyword representation for Elle.
    pub fn keyword(&self) -> &'static str {
        match self {
            MlirPolicy::Off => "off",
            MlirPolicy::Eager => "eager",
            MlirPolicy::Adaptive { .. } => "adaptive",
        }
    }

    /// Parse from a keyword string.
    pub fn from_keyword(s: &str) -> Option<MlirPolicy> {
        match s {
            "off" => Some(MlirPolicy::Off),
            "eager" => Some(MlirPolicy::Eager),
            "adaptive" => Some(MlirPolicy::Adaptive { threshold: 10 }),
            _ => None,
        }
    }
}

// ── Trace keywords ────────────────────────────────────────────────

/// All known trace keywords. Unknown keywords in `--trace=` are rejected;
/// unknown keywords in Elle `(put (vm/config) :trace ...)` are accepted
/// silently (forward compat for :spirv, :mlir, :gpu).
pub const TRACE_KEYWORDS: &[&str] = &[
    "call", "signal", "compile", "fiber", "hir", "lir", "emit", "jit", "io", "gc", "import",
    "macro", "wasm", "capture", "arena", "escape", "bytecode",
    // Future: accepted without error
    "spirv", "mlir", "gpu",
];

// ── Dump keywords ─────────────────────────────────────────────────

/// Compiler-stage dumps requested from `--dump=<kw>,...`. Unlike `--trace=`
/// (which enables runtime logging), `--dump=` runs the compiler up to each
/// requested stage, prints the artifact, and exits without executing.
pub const DUMP_KEYWORDS: &[&str] = &[
    "ast", "hir", "fhir", "lir", "jit", "cfg", "dfa", "defuse", "regions", "git",
];

pub mod dump_bits {
    pub const AST: u32 = 1 << 0;
    pub const HIR: u32 = 1 << 1;
    pub const LIR: u32 = 1 << 2;
    pub const JIT: u32 = 1 << 3;
    pub const CFG: u32 = 1 << 4;
    pub const DFA: u32 = 1 << 5;
    pub const GIT: u32 = 1 << 6;
    pub const FHIR: u32 = 1 << 7;
    pub const DEFUSE: u32 = 1 << 8;
    pub const REGIONS: u32 = 1 << 9;
    pub const ALL: u32 = (1 << 10) - 1;

    /// Convert a keyword name to its bit. Returns 0 for unknown keywords.
    pub fn from_name(name: &str) -> u32 {
        match name {
            "ast" => AST,
            "hir" => HIR,
            "fhir" => FHIR,
            "lir" => LIR,
            "jit" => JIT,
            "cfg" => CFG,
            "dfa" => DFA,
            "git" => GIT,
            "defuse" => DEFUSE,
            "regions" => REGIONS,
            _ => 0,
        }
    }
}

/// Bit positions for trace keywords — avoids HashSet lookups on hot paths.
/// Each keyword maps to a bit in a u32.
pub mod trace_bits {
    pub const CALL: u32 = 1 << 0;
    pub const SIGNAL: u32 = 1 << 1;
    pub const COMPILE: u32 = 1 << 2;
    pub const FIBER: u32 = 1 << 3;
    pub const HIR: u32 = 1 << 4;
    pub const LIR: u32 = 1 << 5;
    pub const EMIT: u32 = 1 << 6;
    pub const JIT: u32 = 1 << 7;
    pub const IO: u32 = 1 << 8;
    pub const GC: u32 = 1 << 9;
    pub const IMPORT: u32 = 1 << 10;
    pub const MACRO: u32 = 1 << 11;
    pub const WASM: u32 = 1 << 12;
    pub const CAPTURE: u32 = 1 << 13;
    pub const ARENA: u32 = 1 << 14;
    pub const ESCAPE: u32 = 1 << 15;
    pub const BYTECODE: u32 = 1 << 16;
    pub const ALL: u32 = (1 << 17) - 1;

    /// Convert a keyword name to its bit. Returns 0 for unknown keywords.
    pub fn from_name(name: &str) -> u32 {
        match name {
            "call" => CALL,
            "signal" => SIGNAL,
            "compile" => COMPILE,
            "fiber" => FIBER,
            "hir" => HIR,
            "lir" => LIR,
            "emit" => EMIT,
            "jit" => JIT,
            "io" => IO,
            "gc" => GC,
            "import" => IMPORT,
            "macro" => MACRO,
            "wasm" => WASM,
            "capture" => CAPTURE,
            "arena" => ARENA,
            "escape" => ESCAPE,
            "bytecode" => BYTECODE,
            // Future keywords — accepted but no bit (traced via HashSet)
            _ => 0,
        }
    }
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
    /// Active compiler-stage dumps (see `DUMP_KEYWORDS`).
    pub dump: HashSet<String>,
    /// Bitfield cache mirroring `dump`.
    pub dump_bits: u32,
    /// Print compilation stats on exit.
    pub stats: bool,
    /// Whether flip instructions are enabled.
    pub flip: bool,
}

impl RuntimeConfig {
    /// Build a RuntimeConfig from the static global Config.
    pub fn from_static_config(config: &Config) -> Self {
        let jit = if config.jit == 0 {
            JitPolicy::Off
        } else if config.jit == 1 {
            JitPolicy::Eager
        } else {
            JitPolicy::Adaptive {
                threshold: (config.jit - 1) as usize,
            }
        };

        let wasm = if config.wasm_full {
            WasmPolicy::Full
        } else if config.wasm > 0 {
            WasmPolicy::Lazy {
                threshold: (config.wasm - 1) as usize,
            }
        } else {
            WasmPolicy::Off
        };

        let mlir = if config.mlir == 0 {
            MlirPolicy::Off
        } else if config.mlir == 1 {
            MlirPolicy::Eager
        } else {
            MlirPolicy::Adaptive {
                threshold: (config.mlir - 1) as usize,
            }
        };

        // Map old debug_* flags to trace keywords
        let mut trace = HashSet::new();
        let mut bits = 0u32;
        if config.debug {
            trace.insert("bytecode".to_string());
            bits |= trace_bits::BYTECODE;
        }
        if config.debug_jit {
            trace.insert("jit".to_string());
            bits |= trace_bits::JIT;
        }
        if config.debug_resume {
            trace.insert("fiber".to_string());
            bits |= trace_bits::FIBER;
        }
        if config.debug_stack {
            trace.insert("call".to_string());
            bits |= trace_bits::CALL;
        }
        if config.debug_wasm {
            trace.insert("wasm".to_string());
            bits |= trace_bits::WASM;
        }

        let mut dump_bits_u = 0u32;
        for kw in &config.dump {
            dump_bits_u |= dump_bits::from_name(kw);
        }

        RuntimeConfig {
            trace,
            trace_bits: bits,
            jit,
            wasm,
            mlir,
            debug_bytecode: config.debug,
            dump: config.dump.clone(),
            dump_bits: dump_bits_u,
            stats: config.stats,
            flip: config.flip_instructions,
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
    }

    /// Check if a trace bit is set (fast path — no HashSet lookup).
    #[inline(always)]
    pub fn has_trace_bit(&self, bit: u32) -> bool {
        self.trace_bits & bit != 0
    }

    /// Check if a dump bit is set.
    #[inline(always)]
    pub fn has_dump_bit(&self, bit: u32) -> bool {
        self.dump_bits & bit != 0
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
            dump: HashSet::new(),
            dump_bits: 0,
            stats: false,
            flip: true,
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
    // -- JIT --
    /// JIT hotness value from `--jit=N`. 0 = disabled, N = threshold is N-1.
    pub jit: u32,

    /// Print compilation stats on exit.
    pub stats: bool,

    // -- MLIR --
    /// MLIR tier value from `--mlir=N`. 0 = disabled, 1 = eager, N = threshold is N-1.
    /// Default: 11 (threshold 10, same as JIT).
    pub mlir: u32,

    // -- WASM --
    /// WASM tier value from `--wasm=N`. 0 = disabled, N = threshold is N-1.
    pub wasm: u32,

    /// Full-module WASM mode (`--wasm=full`).
    pub wasm_full: bool,

    /// Skip stdlib in full-module WASM mode.
    pub wasm_no_stdlib: bool,

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

    // -- Debug (old flags, mapped to RuntimeConfig on VM init) --
    /// Print bytecode before execution.
    pub debug: bool,

    /// Print JIT compilation decisions.
    pub debug_jit: bool,

    /// Print fiber resume traces.
    pub debug_resume: bool,

    /// Print stack operations.
    pub debug_stack: bool,

    /// Print WASM host call traces.
    pub debug_wasm: bool,

    /// Dump WASM module bytes to /tmp/elle-wasm-dump.wasm.
    pub wasm_dump: bool,

    /// Print LIR before WASM emission.
    pub wasm_lir: bool,

    /// Chunk user expressions into sub-thunks (experimental).
    pub wasm_chunk: bool,

    /// Sparse spill: only spill live registers at suspend points.
    /// Reduces code size from O(total_regs * suspend_points) to
    /// O(live_regs * suspend_points). On by default.
    pub wasm_sparse_spill: bool,

    /// Auto-insert `FlipEnter`/`FlipSwap`/`FlipExit` instructions in
    /// lowered functions (Phase 4b). On by default — escape-analysis
    /// gates injection so only safe loops get flip. Disable via
    /// `--flip=off` to fall back to trampoline-only rotation.
    pub flip_instructions: bool,

    /// Compiler stages to dump (from `--dump=kw1,kw2,...`). Valid keywords
    /// are listed in `DUMP_KEYWORDS`. When non-empty, the compiler runs up
    /// to each requested stage, prints its artifact, and exits without
    /// executing.
    pub dump: HashSet<String>,

    /// Trace keywords from `--trace=kw1,kw2,...`.
    /// Stored here from CLI parsing, then merged into RuntimeConfig on VM init.
    pub trace_keywords: Vec<String>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            jit: 11,
            stats: false,
            mlir: 11,
            wasm: 0,
            wasm_full: false,
            wasm_no_stdlib: false,
            cache: default_cache_dir(),
            no_uring: false,
            home: std::env::var("ELLE_HOME").ok(),
            path: std::env::var("ELLE_PATH").ok(),
            json: false,
            debug: false,
            debug_jit: false,
            debug_resume: false,
            debug_stack: false,
            debug_wasm: false,
            wasm_dump: false,
            wasm_lir: false,
            wasm_chunk: false,
            wasm_sparse_spill: true,
            flip_instructions: true,
            dump: HashSet::new(),
            trace_keywords: Vec::new(),
        }
    }
}

impl Config {
    /// Whether JIT compilation is enabled.
    pub fn jit_enabled(&self) -> bool {
        self.jit > 0
    }

    /// JIT hotness threshold (calls before compilation).
    pub fn jit_threshold(&self) -> usize {
        self.jit.saturating_sub(1) as usize
    }

    /// Whether WASM tiered compilation is enabled.
    pub fn wasm_tier_enabled(&self) -> bool {
        self.wasm > 0
    }

    /// WASM tier hotness threshold.
    pub fn wasm_threshold(&self) -> usize {
        self.wasm.saturating_sub(1) as usize
    }

    /// Parse CLI arguments into a Config and remaining positional args.
    ///
    /// Returns `(config, subcommand_or_none, remaining_args)`.
    /// `remaining_args` contains file args and everything after `--`.
    pub fn parse(args: &[String]) -> Result<(Config, Vec<String>), String> {
        let mut config = Config::default();
        let mut remaining = Vec::new();
        let mut i = 0;
        let mut eval_exprs: Vec<String> = Vec::new();

        while i < args.len() {
            let arg = &args[i];

            if arg == "--" {
                // Everything after -- goes to user args
                remaining.push("--".to_string());
                remaining.extend_from_slice(&args[i + 1..]);
                break;
            }

            // --key=value style
            if let Some(rest) = arg.strip_prefix("--jit=") {
                // Named policies: off, eager, adaptive
                match rest {
                    "off" => config.jit = 0,
                    "eager" => config.jit = 1,
                    "adaptive" => config.jit = 11,
                    _ => {
                        config.jit = rest.parse::<u32>().map_err(|_| {
                            format!(
                                "--jit: expected integer or policy name (off/eager/adaptive), got '{}'",
                                rest
                            )
                        })?;
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--mlir=") {
                match rest {
                    "off" => config.mlir = 0,
                    "eager" => config.mlir = 1,
                    "adaptive" => config.mlir = 11,
                    _ => {
                        config.mlir = rest.parse::<u32>().map_err(|_| {
                            format!(
                                "--mlir: expected integer or policy name (off/eager/adaptive), got '{}'",
                                rest
                            )
                        })?;
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--wasm=") {
                match rest {
                    "off" => {
                        config.wasm = 0;
                        config.wasm_full = false;
                    }
                    "full" => config.wasm_full = true,
                    "lazy" => config.wasm = 11,
                    _ => {
                        config.wasm = rest.parse::<u32>().map_err(|_| {
                            format!(
                                "--wasm: expected integer or policy name (off/full/lazy), got '{}'",
                                rest
                            )
                        })?;
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--flip=") {
                config.flip_instructions = match rest {
                    "on" | "true" | "1" => true,
                    "off" | "false" | "0" => false,
                    _ => {
                        return Err(format!("--flip: expected on/off, got '{}'", rest));
                    }
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--trace=") {
                if rest == "all" {
                    for kw in TRACE_KEYWORDS {
                        config.trace_keywords.push(kw.to_string());
                    }
                } else {
                    for kw in rest.split(',') {
                        let kw = kw.trim();
                        if !kw.is_empty() {
                            config.trace_keywords.push(kw.to_string());
                        }
                    }
                }
                // Also set old debug_* flags for backward compat with code
                // that still checks them directly (wasm paths, etc.)
                for kw in &config.trace_keywords {
                    match kw.as_str() {
                        "jit" => config.debug_jit = true,
                        "fiber" => config.debug_resume = true,
                        "call" => config.debug_stack = true,
                        "wasm" => config.debug_wasm = true,
                        "bytecode" => config.debug = true,
                        _ => {}
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--dump=") {
                if rest == "all" {
                    for kw in DUMP_KEYWORDS {
                        config.dump.insert((*kw).to_string());
                    }
                } else {
                    for kw in rest.split(',') {
                        let kw = kw.trim();
                        if kw.is_empty() {
                            continue;
                        }
                        if dump_bits::from_name(kw) == 0 {
                            return Err(format!(
                                "--dump: unknown stage '{}'. Valid: {}",
                                kw,
                                DUMP_KEYWORDS.join(", ")
                            ));
                        }
                        config.dump.insert(kw.to_string());
                    }
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--cache=") {
                config.cache = if rest.is_empty() {
                    None
                } else {
                    Some(rest.to_string())
                };
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--home=") {
                config.home = Some(rest.to_string());
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--path=") {
                config.path = Some(rest.to_string());
                i += 1;
                continue;
            }

            // Boolean flags
            match arg.as_str() {
                "--json" => config.json = true,
                "--stats" => config.stats = true,
                "--wasm-no-stdlib" => config.wasm_no_stdlib = true,
                "--no-uring" => config.no_uring = true,
                // Old debug flags — kept as aliases
                "--debug" => {
                    config.debug = true;
                    config.trace_keywords.push("bytecode".into());
                }
                "--debug-jit" => {
                    config.debug_jit = true;
                    config.trace_keywords.push("jit".into());
                }
                "--debug-resume" => {
                    config.debug_resume = true;
                    config.trace_keywords.push("fiber".into());
                }
                "--debug-stack" => {
                    config.debug_stack = true;
                    config.trace_keywords.push("call".into());
                }
                "--debug-wasm" => {
                    config.debug_wasm = true;
                    config.trace_keywords.push("wasm".into());
                }
                "--wasm-dump" => config.wasm_dump = true,
                "--wasm-lir" => config.wasm_lir = true,
                "--wasm-chunk" => config.wasm_chunk = true,
                "--wasm-no-sparse-spill" => config.wasm_sparse_spill = false,
                "--eval" | "-e" => {
                    i += 1;
                    if i >= args.len() {
                        return Err("--eval requires an argument".to_string());
                    }
                    eval_exprs.push(args[i].clone());
                }
                _ => {
                    // Not a recognized flag — pass through as positional
                    remaining.push(arg.clone());
                }
            }

            i += 1;
        }

        // Prepend eval expressions as synthetic file args
        // They'll be handled specially in main
        for expr in eval_exprs.into_iter().rev() {
            remaining.insert(0, format!("--eval:{}", expr));
        }

        Ok((config, remaining))
    }
}
