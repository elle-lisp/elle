//! Compilation policies (JIT, WASM, MLIR) parsed from CLI flags.

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
            JitPolicy::Custom => 0,
        }
    }

    /// Keyword representation for Elle.
    pub fn keyword(&self) -> &'static str {
        match self {
            JitPolicy::Off => "off",
            JitPolicy::Eager => "eager",
            JitPolicy::Adaptive { .. } => "adaptive",
            JitPolicy::Custom => "custom",
        }
    }

    /// Parse from a keyword string.
    pub fn from_keyword(s: &str) -> Option<JitPolicy> {
        match s {
            "off" => Some(JitPolicy::Off),
            "eager" => Some(JitPolicy::Eager),
            "adaptive" => Some(JitPolicy::Adaptive { threshold: 10 }),
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
}

impl WasmPolicy {
    pub fn keyword(&self) -> &'static str {
        match self {
            WasmPolicy::Off => "off",
            WasmPolicy::Full => "full",
            WasmPolicy::Lazy { .. } => "lazy",
        }
    }

    pub fn from_keyword(s: &str) -> Option<WasmPolicy> {
        match s {
            "off" => Some(WasmPolicy::Off),
            "full" => Some(WasmPolicy::Full),
            "lazy" => Some(WasmPolicy::Lazy { threshold: 10 }),
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
