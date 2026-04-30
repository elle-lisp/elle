//! Intrinsic operation mapping for operator specialization.
//!
//! Maps known primitive operator SymbolIds to specialized LIR instructions
//! (BinOp, CmpOp, UnaryOp) so the lowerer can emit them directly instead
//! of generic LoadGlobal + Call sequences.
//!
//! Also provides `build_immediate_primitives` — a set of primitive names
//! whose return value is guaranteed to be an immediate (int, float,
//! bool, nil, keyword, symbol). Used by escape analysis (`result_is_safe`)
//! to accept calls to these primitives in scope-allocated let bodies.

use super::types::ConvOp;
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use rustc_hash::{FxHashMap, FxHashSet};

/// A known intrinsic operation that can be compiled to specialized instructions.
#[derive(Debug, Clone, Copy)]
pub(crate) enum IntrinsicOp {
    Conversion(ConvOp),
}

/// Build the intrinsics map from a symbol table.
///
/// Maps SymbolId to IntrinsicOp for known primitive operations.
/// Only includes operators that are registered as global primitives
/// and whose semantics match the corresponding LIR instruction exactly.
pub(crate) fn build_intrinsics(symbols: &SymbolTable) -> FxHashMap<SymbolId, IntrinsicOp> {
    let mut map = FxHashMap::default();

    let mut add = |name: &str, op: IntrinsicOp| {
        if let Some(id) = symbols.get(name) {
            map.insert(id, op);
        }
    };

    // Conversion (1-arg calls lower to Convert; 2-arg integer/int falls through to Call)
    add("float", IntrinsicOp::Conversion(ConvOp::IntToFloat));
    add("integer", IntrinsicOp::Conversion(ConvOp::FloatToInt));
    add("int", IntrinsicOp::Conversion(ConvOp::FloatToInt));

    map
}

/// Primitives guaranteed to return immediates.
///
/// Every name here has been verified: on success, the primitive returns
/// `Value::int(...)`, `Value::float(...)`, `Value::bool(...)`, or
/// `Value::keyword(...)` — all immediates that `RegionExit`
/// will not free. On error, primitives return `(SIG_ERROR, ...)` which
/// propagates via the signal mechanism, never as a normal return value.
///
/// **Exclusions (look safe but aren't):**
/// - `min`, `max`: return their input (`args[0]`) unmodified on 1-arg
///   calls, which could be a heap value.
/// - `first`, `rest`, `get`, `last`, `butlast`: return arbitrary values.
/// - `number->string`, `int->string`, `symbol->string`: return strings.
const IMMEDIATE_PRIMITIVES: &[&str] = &[
    // Type predicates → bool
    "nil?",
    "pair?",
    "list?",
    "number?",
    "symbol?",
    "string?",
    "boolean?",
    "keyword?",
    "array?",
    "struct?",
    "bytes?",
    "ptr?",
    "pointer?",
    "fiber?",
    "closure?",
    "jit?",
    "silent?",
    "coro?",
    "box?",
    // Collection predicates → bool
    "empty?",
    "has?",
    "contains?",
    // String predicates → bool (canonical + aliases)
    "string/contains?",
    "string-contains?",
    "string/starts-with?",
    "string-starts-with?",
    "string/ends-with?",
    "string-ends-with?",
    // Numeric predicates → bool
    "integer?",
    "float?",
    "even?",
    "odd?",
    // Closure introspection predicates → bool (canonical + aliases)
    "fn/mutates-params?",
    "mutates-params?",
    "fn/errors?",
    "coroutine?",
    // Collection → int
    "length",
    // Numeric → int or float
    "abs",
    "floor",
    "ceil",
    "round",
    // Type conversion → int or float
    "float",
    "integer",
    "int",
    "parse-int",
    "parse-float",
    // Type introspection → keyword
    "type",
    "type-of",
    // Arena introspection → int (via SIG_QUERY)
    "arena/count",
    "arena-count",
    "arena/bytes",
    // Closure introspection → int or nil (canonical + aliases)
    "fn/bytecode-size",
    "bytecode-size",
    "fn/captures",
    "captures",
    // String → int or nil (nil is also immediate)
    "string/find",
    "string-find",
    "string-index",
    "string/index",
    // Identity → bool
    "identical?",
    // Port predicates → bool
    "port?",
    "port/open?",
    // Parameter predicate → bool
    "parameter?",
    // Math constants → float
    "math/pi",
    "pi",
    "math/e",
    "e",
    "math/inf",
    "+inf",
    "inf",
    "math/-inf",
    "-inf",
    "math/nan",
    "nan",
];

/// Primitives that store their argument(s) into external mutable data
/// structures. Calls to these with non-immediate arguments can cause
/// heap values to escape the current tail-call iteration.
///
/// Used by rotation-safety analysis: a tail-call loop containing calls
/// to any of these with a heap argument is not safe for pool rotation.
#[allow(dead_code)]
const MUTATING_PRIMITIVES: &[&str] = &["push", "put", "del", "pop", "fiber/resume", "assign"];

/// Build the set of primitive SymbolIds known to return immediates.
///
/// Used by escape analysis to accept `(let (...) (length x))` and
/// similar patterns where the body result is a call to one of these
/// primitives.
pub(crate) fn build_immediate_primitives(symbols: &SymbolTable) -> FxHashSet<SymbolId> {
    let mut set = FxHashSet::default();
    for &name in IMMEDIATE_PRIMITIVES {
        if let Some(id) = symbols.get(name) {
            set.insert(id);
        }
    }
    set
}

/// Primitives that store an argument into a collection, causing the arg
/// value to escape the current scope. Used by `walk_for_outward_set` to
/// reject while loops where a heap-allocated value would be pushed into
/// an outer collection that outlives the per-iteration scope.
///
/// Unlike MUTATING_PRIMITIVES (which includes fiber/resume, del, pop, etc.),
/// these specifically INSERT a value into a live collection.
const ARG_ESCAPING_PRIMITIVES: &[&str] = &["push", "put"];

/// Build the set of primitive SymbolIds that insert args into collections.
pub(crate) fn build_arg_escaping_primitives(symbols: &SymbolTable) -> FxHashSet<SymbolId> {
    let mut set = FxHashSet::default();
    for &name in ARG_ESCAPING_PRIMITIVES {
        if let Some(id) = symbols.get(name) {
            set.insert(id);
        }
    }
    set
}

/// Primitives that return existing heap values without allocating.
///
/// These are accessors: they return a value that was already alive before
/// the call. `rest` returns an existing cons cell, `first` returns a
/// pre-existing element, `get` returns an element from a collection.
///
/// Used by `walk_for_outward_set`: an `(assign outer-binding (rest x))`
/// is safe because the returned value wasn't allocated in the current scope
/// or iteration — it predates the scope, so RegionExit / FlipSwap won't
/// free it.
/// Note: fiber/resume and coro/resume return values from the child's
/// outbox (parent-owned arena), not from the current iteration's arena.
/// FlipSwap won't free them, so they're safe for the same reason as
/// accessors: the returned value predates the current scope.
const NON_ALLOCATING_ACCESSORS: &[&str] = &[
    "first",
    "rest",
    "first",
    "rest",
    "get",
    "last",
    "fiber/resume",
    "coro/resume",
];

/// Build the set of primitive SymbolIds that return pre-existing values.
pub(crate) fn build_non_allocating_accessors(symbols: &SymbolTable) -> FxHashSet<SymbolId> {
    let mut set = FxHashSet::default();
    for &name in NON_ALLOCATING_ACCESSORS {
        if let Some(id) = symbols.get(name) {
            set.insert(id);
        }
    }
    set
}

/// Stdlib functions known to not escape heap values to external structures.
///
/// These are higher-order functions implemented in Elle (not native
/// primitives) that are pure: they create new collections and return
/// them, without storing arguments into external mutable structures.
///
/// Used by `walk_for_outward_set` and `body_escapes_heap_values` to
/// treat calls to these as safe (equivalent to rotation-safe callees).
const NON_ESCAPING_STDLIB: &[&str] = &[
    "map",
    "filter",
    "reduce",
    "fold",
    "zip",
    "flat-map",
    "take",
    "drop",
    "reverse",
    "sort",
    "sort-by",
    "range",
    "repeat",
    "interleave",
    "partition",
    "group-by",
    "frequencies",
    "distinct",
    "flatten",
    "take-while",
    "drop-while",
    "some?",
    "every?",
    "none?",
    "find",
    "count",
    "sum",
    "product",
    "min-by",
    "max-by",
];

/// Build the set of stdlib SymbolIds known to not escape heap values.
pub(crate) fn build_non_escaping_stdlib(symbols: &SymbolTable) -> FxHashSet<SymbolId> {
    let mut set = FxHashSet::default();
    for &name in NON_ESCAPING_STDLIB {
        if let Some(id) = symbols.get(name) {
            set.insert(id);
        }
    }
    set
}

/// Build call classification data for region inference.
pub(crate) fn build_call_classification(symbols: &SymbolTable) -> crate::hir::CallClassification {
    crate::hir::CallClassification {
        immediate_primitives: build_immediate_primitives(symbols),
        intrinsic_ops: build_intrinsics(symbols).keys().copied().collect(),
        ..Default::default()
    }
}

/// Build the set of primitive SymbolIds that store heap values externally.
#[allow(dead_code)]
pub(crate) fn build_mutating_primitives(symbols: &SymbolTable) -> FxHashSet<SymbolId> {
    let mut set = FxHashSet::default();
    for &name in MUTATING_PRIMITIVES {
        if let Some(id) = symbols.get(name) {
            set.insert(id);
        }
    }
    set
}
