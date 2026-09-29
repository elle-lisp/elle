// audited: 2026-09-29
//! The compilation policies of the three optimizing tiers, and the one each build starts with.
//!
//! docs/config.md

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
