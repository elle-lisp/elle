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
/// `vocabulary_is_collision_free` proves the list injective under the hash.
/// A missing entry is a defect with no compile error behind it: the keyword
/// prints as `#<keyword:hash>`, and `json/serialize` refuses any struct that
/// carries it as a key, because JSON has no rendering for a hash. Two tests
/// keep the list complete, since the constructor cannot —
/// `vocabulary_covers_literal_mint_sites` scans every form that hands a
/// literal to a keyword constructor, and `vocabulary_covers_accessor_mint_sites`
/// enumerates the tables whose `&'static str` accessors feed the same
/// constructors from behind a `match` arm no scan can see.
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
    "input",
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
    "spec",
    "stats",
    "term",
    "term-display",
    "term-kind",
    "term-span",
    "tier",
    "value",
    "x",
    // File metadata keys (`file/stat`, `file/lstat`)
    "accessed",
    "blksize",
    "created",
    "dev",
    "file-type",
    "gid",
    "inode",
    "is-dir",
    "is-file",
    "is-symlink",
    "modified",
    "nlinks",
    "permissions",
    "rdev",
    "readonly",
    "uid",
    // Compile-query and rewrite keys (`compile/signal`, `compile/bindings`,
    // `compile/call-graph`, `compile/diagnostics`, `compile/rename`)
    "bits",
    "callees",
    "callers",
    "captures",
    "edits",
    "immutable",
    "jit-eligible",
    "leaves",
    "lines",
    "mutated",
    "needs-lbox",
    "new-function",
    "nodes",
    "propagates",
    "roots",
    "rule",
    "safe",
    "scope",
    "severity",
    "shared-captures",
    "silent",
    "source",
    "suggestions",
    "tail",
    "usages",
    "wraps",
    "yields",
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
    // Signal names the registry pre-registers. A signal a program declares at
    // run time cannot be listed here; it is learned where the registry is read
    // (docs/impl/symbol.md § "The display memo").
    "debug",
    "exec",
    "fs",
    "fuel",
    "io",
    "os-signal",
    "wait",
    "yield",
    // Error kinds (`ctx.error(kind, …)` becomes the `:error` field's keyword)
    "argument-error",
    "assertion-failed",
    "arity-error",
    "compile-error",
    "division-by-zero",
    "dns-error",
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
    "task-error",
    "thread-error",
    "tier-rejected",
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
    // External type names: the `ctx.external(type_name, …)` a primitive wraps
    // its handle in, which is what `type-of` hands back for that handle.
    // "port" and "process" appear above.
    "analysis",
    "chan/receiver",
    "chan/sender",
    "fs-watcher",
    "io-backend",
    "io-request",
    "signal-receiver",
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
    // Execution tiers (`VM::active_tier`, reported by `(vm/tier)`)
    "bytecode",
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
    "module",
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

/// Whether `VOCABULARY` carries `name`. `const fn`, so a caller can ask in a
/// `const` context and turn the answer into a build error; the linear scan is
/// what makes it one (the `static_keyword_name` index is not const-buildable).
pub const fn is_vocabulary(name: &str) -> bool {
    let hash = keyword_hash(name);
    let mut i = 0;
    while i < VOCABULARY.len() {
        if keyword_hash(VOCABULARY[i]) == hash {
            return true;
        }
        i += 1;
    }
    false
}

/// `name`, having asserted the vocabulary carries it.
///
/// The spelling of a keyword the runtime coins from a fixed string has to be
/// in `VOCABULARY` or the keyword has no name to print, and `json/serialize`
/// refuses any struct that carries it as a key. Called from a `const` block,
/// this moves that requirement to compile time: `const { vocab("input") }` is
/// a build error at the line that wrote it until "input" is listed.
///
/// `rich_error!` is the caller that needs it. A field name written
/// `input = …` reaches the keyword constructor through `stringify!`, as a
/// token rather than as a string, so no scan of the source can find it
/// (docs/impl/symbol.md § "A spelling the runtime itself mints").
pub const fn vocab(name: &'static str) -> &'static str {
    assert!(
        is_vocabulary(name),
        "keyword spelling missing from VOCABULARY in src/value/keyword.rs"
    );
    name
}

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
