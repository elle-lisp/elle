//! Global configuration parsed from CLI arguments.
//!
//! Set once at startup via `init`, read anywhere via `get`.
//! Replaces all `ELLE_*` environment variables.

use std::sync::OnceLock;

static CONFIG: OnceLock<Config> = OnceLock::new();

/// Read the global config. Returns default if `init` hasn't been called.
pub fn get() -> &'static Config {
    CONFIG.get_or_init(Config::default)
}

/// Initialize the global config. Must be called before `get` for
/// CLI-parsed values to take effect. No-op if already initialized.
pub fn init(config: Config) {
    let _ = CONFIG.set(config);
}

/// All runtime configuration for Elle.
///
/// ## `--jit=N`
///
/// Controls JIT compilation threshold:
/// - `0` — JIT disabled
/// - `N` — JIT enabled, compile after N-1 calls
///   (so `--jit=1` compiles on first call, `--jit=11` compiles after 10)
///
/// Default: 11 (threshold 10, matching the old `ELLE_JIT_THRESHOLD=10`).
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

    // -- WASM --
    /// WASM tier value from `--wasm=N`. 0 = disabled, N = threshold is N-1.
    pub wasm: u32,

    /// Full-module WASM mode (`--wasm=full`).
    pub wasm_full: bool,

    /// Skip stdlib in full-module WASM mode.
    pub wasm_no_stdlib: bool,

    /// Disk cache directory (WASM compilation, future uses).
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

    // -- Debug --
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
}

impl Default for Config {
    fn default() -> Self {
        Config {
            jit: 11,
            stats: false,
            wasm: 0,
            wasm_full: false,
            wasm_no_stdlib: false,
            cache: std::env::var("ELLE_CACHE").ok(),
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
                config.jit = rest
                    .parse::<u32>()
                    .map_err(|_| format!("--jit: expected integer, got '{}'", rest))?;
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--wasm=") {
                if rest == "full" {
                    config.wasm_full = true;
                } else {
                    config.wasm = rest.parse::<u32>().map_err(|_| {
                        format!("--wasm: expected integer or 'full', got '{}'", rest)
                    })?;
                }
                i += 1;
                continue;
            }
            if let Some(rest) = arg.strip_prefix("--cache=") {
                config.cache = Some(rest.to_string());
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
                "--debug" => config.debug = true,
                "--debug-jit" => config.debug_jit = true,
                "--debug-resume" => config.debug_resume = true,
                "--debug-stack" => config.debug_stack = true,
                "--debug-wasm" => config.debug_wasm = true,
                "--wasm-dump" => config.wasm_dump = true,
                "--wasm-lir" => config.wasm_lir = true,
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
