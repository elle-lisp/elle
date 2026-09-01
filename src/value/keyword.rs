//! Keyword identity and the static keyword vocabulary.
//!
//! A keyword's payload is the 64-bit FNV-1a hash of its name — the same
//! function symbol identity uses ([`crate::namehash`]), so equality is
//! `u64 == u64` with no string comparison and no heap dereference, and the
//! same name yields the same payload in every thread, process, and build.
//!
//! Construction is identity only: [`Value::keyword`](crate::value::Value)
//! records nothing. Display resolves a spelling through the per-instance
//! memo first (`SymbolTable::keyword_name`), then through `VOCABULARY` —
//! the build-fixed list of every spelling the Rust runtime itself mints.
//! The vocabulary is the keyword analogue of the primitive-name index in
//! `primitives::registration::static_name`: fixed at build time, immutable,
//! and needing no instance. A spelling in neither renders as
//! `#<keyword:hash>` (docs/impl/symbol.md § "Reading a name, and not
//! reading one").

use crate::symbol::SymbolTable;
use std::collections::HashMap;
use std::sync::LazyLock;

/// The 64-bit name hash of a keyword name — [`crate::namehash::name_hash`],
/// the same function symbol identity uses.
pub const fn keyword_hash(name: &str) -> u64 {
    crate::namehash::name_hash(name)
}

/// Every keyword spelling the Rust runtime mints from a fixed string —
/// result-struct keys, error kinds, type names, config and event keywords.
/// Display-only: identity never consults this list. Spellings that reach a
/// keyword dynamically (read from source, converted from a string, minted by
/// a plugin, parsed from JSON) are learned by the instance memo instead.
///
/// `vocabulary_is_collision_free` proves the list injective under the hash;
/// a missing entry is not an error — the keyword prints as `#<keyword:hash>`
/// until the spelling is added here or learned at run time.
static VOCABULARY: &[&str] = &[
    // Result-struct keys and general fields
    "a",
    "active-allocator",
    "addr",
    "aliases",
    "allocated-bytes",
    "annotated",
    "args",
    "arity",
    "attempts",
    "blocks",
    "calls",
    "category",
    "code",
    "col",
    "count",
    "cwd",
    "data",
    "doc",
    "edges",
    "entry",
    "env",
    "example",
    "file",
    "first",
    "func",
    "id",
    "instrs",
    "k",
    "kind",
    "label",
    "last",
    "length",
    "line",
    "locals",
    "message",
    "name",
    "params",
    "path",
    "peak-count",
    "pid",
    "port",
    "process",
    "rc",
    "reason",
    "region",
    "regs",
    "rest",
    "scope-depth",
    "scope-dtor-count",
    "scope-enter-count",
    "sender-pid",
    "sender-uid",
    "signal",
    "spans",
    "stats",
    "term",
    "term-display",
    "term-kind",
    "term-span",
    "value",
    "x",
    // Status and outcome keywords
    "denied",
    "disconnected",
    "empty",
    "error",
    "failed",
    "flip",
    "full",
    "ineligible",
    "missing",
    "ok",
    "pending",
    "ready",
    "recursive",
    "gated",
    // Capability / feature gating
    "capability-denied",
    "feature-disabled",
    "fiber/caps",
    // Error kinds (`ctx.error(kind, …)` becomes the `:error` field's keyword)
    "argument-error",
    "arity-error",
    "compile-error",
    "division-by-zero",
    "double-free",
    "encoding-error",
    "exec-error",
    "ffi-error",
    "fiber-error",
    "format-error",
    "internal-error",
    "io-error",
    "match-error",
    "mlir-error",
    "os-signal-error",
    "overflow-error",
    "range-error",
    "read-error",
    "rewrite-error",
    "runtime-error",
    "serde-error",
    "state-error",
    "thread-error",
    "trait-error",
    "type-error",
    "unknown-tier",
    "allocation-error",
    "analysis-error",
    "barrier-error",
    "eval-error",
    "lookup-error",
    "parse-error",
    "signal-error",
    "syntax-error",
    "value-error",
    "stack-overflow",
    "out-of-fuel",
    "halt",
    "double-resume",
    "failed-assertion",
    "not-implemented",
    "timed-out",
    // Type names (`type-of` and type errors mint `Value::keyword(type_name())`)
    "boolean",
    "float",
    "integer",
    "keyword",
    "list",
    "native-fn",
    "nil",
    "ptr",
    "symbol",
    "unknown",
    "array",
    "@array",
    "box",
    "bytes",
    "@bytes",
    "capture-cell",
    "closure",
    "closure-template",
    "external",
    "ffi",
    "ffi-signature",
    "ffi-type",
    "fiber",
    "library-handle",
    "parameter",
    "set",
    "@set",
    "string",
    "@string",
    "struct",
    "@struct",
    "syntax",
    "thread-handle",
    // Trait protocol keys
    "Collection",
    "Sequence",
    "conj",
    "display",
    "empty?",
    "has?",
    "iter",
    "nth",
    // Compile / VM / config keywords
    "compile/barrier-module",
    "compile/dumps",
    "compile/run-on",
    "compile/whole-module",
    "compile/whole-module-syntax",
    "debug-bytecode",
    "list-primitives",
    "primitive",
    "primitive-meta",
    "vm/config",
    "vm/config-set",
    "trace",
    "unicode",
    "adaptive",
    "custom",
    "eager",
    "lazy",
    "off",
    "on",
    // Execution tiers
    "git",
    "gpu",
    "jit",
    "jit?",
    "jit/rejections",
    "mlir",
    "mlir-cpu",
    "mlir/compile-spirv",
    "vm",
    "wasm",
    // Arena / memory gauges
    "arena/allocs",
    "arena/stats",
    "object-count",
    "object-limit",
    "objects",
    // I/O and process keywords
    "read-write",
    "timeout",
    "read",
    "write",
    "inherit",
    "null",
    "pipe",
    "async",
    "keys",
    "deny",
    "from",
    "start",
    "current",
    "end",
    "text",
    "binary",
    "keepalive",
    "encoding",
    "sigterm",
    "sigkill",
    "sighup",
    "sigint",
    "sigquit",
    "sigpipe",
    "sigalrm",
    "sigusr1",
    "sigusr2",
    "sigchld",
    "sigcont",
    "sigstop",
    "sigtstp",
    "sigttin",
    "sigttou",
    "sigwinch",
    "stderr",
    "stdin",
    "stdout",
    // File-watch event kinds
    "create",
    "modify",
    "remove",
    "rename",
    // FFI type keywords (`TypeDesc::from_keyword`; "float"/"string" appear
    // above)
    "bool",
    "char",
    "default",
    "double",
    "i16",
    "i32",
    "i64",
    "i8",
    "int",
    "long",
    "short",
    "size",
    "ssize",
    "u16",
    "u32",
    "u64",
    "u8",
    "uchar",
    "uint",
    "ulong",
    "ushort",
    "void",
    // Fiber statuses
    "alive",
    "dead",
    "new",
    "paused",
    // Introspection enums: binding scope, capture kind, diagnostic severity,
    // symbol kind, LIR terminator kind, compile-dump kind ("parameter",
    // "error", "jit" already appear above)
    "local",
    "lbox",
    "transitive",
    "info",
    "warning",
    "function",
    "variable",
    "builtin",
    "macro",
    "branch",
    "emit",
    "jump",
    "return",
    "unreachable",
    "ast",
    "cfg",
    "defuse",
    "dfa",
    "escape",
    "fhir",
    "hir",
    "lir",
    "regions",
];

/// The spelling of a vocabulary keyword, or `None` if `hash` names nothing
/// the runtime spells itself.
pub(crate) fn static_keyword_name(hash: u64) -> Option<&'static str> {
    static INDEX: LazyLock<HashMap<u64, &'static str>> = LazyLock::new(|| {
        let mut index = HashMap::new();
        for name in VOCABULARY {
            index.insert(keyword_hash(name), *name);
        }
        index
    });
    INDEX.get(&hash).copied()
}

/// Resolve a keyword payload to its spelling: the instance memo first, then
/// the static vocabulary. The one lookup order every display and
/// name-recovering site uses.
pub(crate) fn resolve_keyword_name(memo: Option<&SymbolTable>, hash: u64) -> Option<&str> {
    memo.and_then(|m| m.keyword_name(hash))
        .or_else(|| static_keyword_name(hash))
}

#[cfg(test)]
mod tests;
