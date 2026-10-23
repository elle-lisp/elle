// audited: 2026-10-23
//! The compilation policies of the three optimizing tiers, the graft and the JIT verifier, and the one each build starts with.
//!
//! docs/config.md
//! docs/impl/verifier.md

// ── JIT policy ────────────────────────────────────────────────────

/// JIT compilation policy.
#[derive(Debug, Clone, PartialEq)]
pub enum JitPolicy {
    /// JIT disabled.
    Off,
    /// Compile on first call.
    Eager,
    /// Compile after N calls.
    Adaptive { threshold: usize },
}

impl JitPolicy {
    /// The policy a build starts with: adaptive at ten calls where the JIT is
    /// the build's tier, and off where the build carries none or another
    /// (docs/config.md).
    pub fn build_default() -> Self {
        if cfg!(all(
            feature = "jit",
            not(feature = "mlir"),
            not(feature = "wasm")
        )) {
            JitPolicy::Adaptive { threshold: 10 }
        } else {
            JitPolicy::Off
        }
    }

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
        }
    }

    /// What `(vm/config :jit)` reads: nil when off, 0 when eager, the count
    /// otherwise.
    pub fn reading(&self) -> Option<usize> {
        self.enabled().then(|| self.threshold())
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
}

impl WasmPolicy {
    /// What `(vm/config :wasm)` reads in a `wasm` build.
    pub fn keyword(&self) -> &'static str {
        match self {
            WasmPolicy::Off => "off",
            WasmPolicy::Full => "full",
            WasmPolicy::Lazy { .. } => "lazy",
        }
    }
}

// ── MLIR policy ──────────────────────────────────────────────────

/// MLIR compilation policy for GPU-eligible functions.
///
/// When the `mlir` feature is compiled in, GPU-eligible functions are compiled
/// through MLIR → LLVM, and this policy controls when that compilation happens.
#[derive(Debug, Clone, PartialEq)]
pub enum MlirPolicy {
    /// MLIR disabled.
    Off,
    /// Compile on first eligible call.
    Eager,
    /// Compile after N calls.
    Adaptive { threshold: usize },
}

impl MlirPolicy {
    /// The policy a build starts with: adaptive at ten calls where MLIR is the
    /// build's tier, and off everywhere else (docs/config.md).
    pub fn build_default() -> Self {
        if cfg!(all(feature = "mlir", not(feature = "wasm"))) {
            MlirPolicy::Adaptive { threshold: 10 }
        } else {
            MlirPolicy::Off
        }
    }

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

    /// What `(vm/config :mlir)` reads: nil when off, 0 when eager, the count
    /// otherwise.
    pub fn reading(&self) -> Option<usize> {
        self.enabled().then(|| self.threshold())
    }
}

// ── Graft policy ─────────────────────────────────────────────────

/// Whether a call to a known function becomes its body (docs/impl/graft.md).
/// `Off` keeps every call a call in the units an instance compiles after its
/// boot; only the rig and a Rust test set it.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GraftPolicy {
    /// Graft every call that qualifies.
    #[default]
    On,
    /// Keep every call a call.
    Off,
}

// ── The JIT verifier ─────────────────────────────────────────────

/// Whether Cranelift checks the IR of each function the JIT compiles before
/// the backend compiles it (docs/impl/verifier.md). A test turns it on.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum JitVerifier {
    /// Verify each function.
    On,
    /// Compile each function unverified.
    #[default]
    Off,
}

impl JitVerifier {
    /// The variable that carries the setting to every process a test starts.
    pub const VAR: &'static str = "ELLE_JIT_VERIFY";

    /// `on` or `off`, the two words the flag and the variable take.
    fn from_word(word: &str) -> Option<Self> {
        match word {
            "on" => Some(JitVerifier::On),
            "off" => Some(JitVerifier::Off),
            _ => None,
        }
    }

    /// The setting `--jit-verify=WORD` names, or why the flag is refused.
    pub(crate) fn from_flag(word: &str) -> Result<Self, String> {
        Self::from_word(word)
            .ok_or_else(|| format!("--jit-verify: expected on or off, got '{word}'"))
    }

    /// The setting the variable's `value` names. Unset and empty are off, so
    /// an exported empty variable leaves the default alone.
    pub fn from_var(value: Option<&str>) -> Result<Self, String> {
        match value {
            None | Some("") => Ok(JitVerifier::Off),
            Some(word) => Self::from_word(word)
                .ok_or_else(|| format!("{}: expected on or off, got '{word}'", Self::VAR)),
        }
    }

    /// The setting this process's environment names.
    pub fn from_env() -> Result<Self, String> {
        let value = std::env::var_os(Self::VAR).map(|v| v.to_string_lossy().into_owned());
        Self::from_var(value.as_deref())
    }

    /// Whether Cranelift verifies.
    pub fn is_on(self) -> bool {
        self == JitVerifier::On
    }
}
