//! Trace and dump keyword tables and their bit encodings.

// ── Trace keywords ────────────────────────────────────────────────

/// All known trace keywords. Unknown keywords in `--trace=` are rejected;
/// unknown keywords in Elle `(put (vm/config) :trace ...)` are accepted
/// silently (forward compat for :spirv, :mlir, :gpu).
pub const TRACE_KEYWORDS: &[&str] = &[
    "call",
    "signal",
    "compile",
    "fiber",
    "hir",
    "lir",
    "emit",
    "jit",
    "io",
    "import",
    "macro",
    "wasm",
    "capture",
    "arena",
    "escape",
    "bytecode",
    "posix",
    "chan",
    "rc",
    "regions",
    "anf",
    "pages", // Future: accepted without error
    "spirv",
    "mlir",
    "gpu",
    // Region/free diagnostics (see src/value/fiberheap/freelog.rs).
    "free",
    "guardfree",
    "freebt",
    "scrub",
    // Park/resume diagnostics: log every suspended-frame park (JIT side-exit
    // helpers) and every frame replay (resume_suspended) with the frame's
    // shape and value types. See src/jit/suspend.rs and src/vm/core/resume.rs.
    "park",
    // JIT diagnostics: force synchronous Cranelift compilation on the VM
    // thread instead of the background `elle-jit` worker (see
    // src/vm/jit_entry.rs `submit_jit_task`). Splits install-race bugs from
    // codegen bugs: a failure that persists under `syncjit` is in codegen or
    // its inputs; one that vanishes lives at the worker boundary.
    "syncjit",
];

// ── Dump keywords ─────────────────────────────────────────────────

/// Compiler-stage dumps requested from `--dump=<kw>,...`. Unlike `--trace=`
/// (which enables runtime logging), `--dump=` runs the compiler up to each
/// requested stage, prints the artifact, and exits without executing.
pub const DUMP_KEYWORDS: &[&str] = &[
    "ast", "hir", "fhir", "lir", "jit", "cfg", "dfa", "defuse", "regions", "escape", "git",
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
    pub const ESCAPE: u32 = 1 << 10;
    pub const ALL: u32 = (1 << 11) - 1;

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
            "escape" => ESCAPE,
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
    pub const IMPORT: u32 = 1 << 10;
    pub const MACRO: u32 = 1 << 11;
    pub const WASM: u32 = 1 << 12;
    pub const CAPTURE: u32 = 1 << 13;
    pub const ARENA: u32 = 1 << 14;
    pub const ESCAPE: u32 = 1 << 15;
    pub const BYTECODE: u32 = 1 << 16;
    /// POSIX-signal subsystem (os/sig-* primitives, signalfd / kqueue
    /// EVFILT_SIGNAL plumbing, threadpool blocking sig reads). Read via
    /// `sigfd::posix_trace`, which takes the instance's trace cell — threaded
    /// from the `SignalReceiver` (captured at `os/sig-watch`), a `NativeCtx`'s
    /// heap, or a `PoolOp` that carried it onto a worker thread; these sites
    /// have no VM reference (src/io/sigfd.rs).
    pub const POSIX: u32 = 1 << 17;
    /// Channel wake protocol (`chan/wait-ready` register/deregister,
    /// `chan/send` wake_all, wake-fd write/close). Read through the
    /// channel's `WakeList`-carried trace cell (a clone of the creating
    /// instance's), so a cross-thread `chan/send` (from a `sys/spawn`'d OS
    /// thread with no `&VM`) still gates on the right instance.
    pub const CHAN: u32 = 1 << 18;
    pub const RC: u32 = 1 << 19;
    pub const REGIONS: u32 = 1 << 20;
    pub const ANF: u32 = 1 << 21;
    pub const PAGES: u32 = 1 << 22;
    /// Every defined bit OR'd together. Built from the constants (not
    /// `(1 << N) - 1`) so the retired `gc` bit (the now-unused `1 << 9`
    /// slot, left between `IO` and `IMPORT`) and any future gaps stay
    /// out of `--trace=all`. Keep in sync with the constants above —
    /// `trace_all_covers_every_defined_bit` guards against drift.
    pub const ALL: u32 = CALL
        | SIGNAL
        | COMPILE
        | FIBER
        | HIR
        | LIR
        | EMIT
        | JIT
        | IO
        | IMPORT
        | MACRO
        | WASM
        | CAPTURE
        | ARENA
        | ESCAPE
        | BYTECODE
        | POSIX
        | CHAN
        | RC
        | REGIONS
        | ANF
        | PAGES;

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
            "import" => IMPORT,
            "macro" => MACRO,
            "wasm" => WASM,
            "capture" => CAPTURE,
            "arena" => ARENA,
            "escape" => ESCAPE,
            "bytecode" => BYTECODE,
            "posix" => POSIX,
            "chan" => CHAN,
            "rc" => RC,
            "regions" => REGIONS,
            "anf" => ANF,
            "pages" => PAGES,
            // Future keywords — accepted but no bit (traced via HashSet)
            _ => 0,
        }
    }
}
