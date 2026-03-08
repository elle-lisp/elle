//! Intrinsic operation mapping for operator specialization.
//!
//! Maps known primitive operator SymbolIds to specialized LIR instructions
//! (BinOp, CmpOp, UnaryOp) so the lowerer can emit them directly instead
//! of generic LoadGlobal + Call sequences.
//!
//! Also provides `build_immediate_primitives` — a set of primitive names
//! whose return value is guaranteed to be a NaN-boxed immediate (int, float,
//! bool, nil, keyword, symbol). Used by escape analysis (`result_is_safe`)
//! to accept calls to these primitives in scope-allocated let bodies.

use super::types::{BinOp, CmpOp, UnaryOp};
use crate::symbol::SymbolTable;
use crate::value::SymbolId;
use rustc_hash::{FxHashMap, FxHashSet};

/// A known intrinsic operation that can be compiled to specialized instructions.
#[derive(Debug, Clone, Copy)]
pub enum IntrinsicOp {
    Binary(BinOp),
    Compare(CmpOp),
    Unary(UnaryOp),
}

/// Build the intrinsics map from a symbol table.
///
/// Maps SymbolId to IntrinsicOp for known primitive operations.
/// Only includes operators that are registered as global primitives
/// and whose semantics match the corresponding LIR instruction exactly.
pub fn build_intrinsics(symbols: &SymbolTable) -> FxHashMap<SymbolId, IntrinsicOp> {
    let mut map = FxHashMap::default();

    let mut add = |name: &str, op: IntrinsicOp| {
        if let Some(id) = symbols.get(name) {
            map.insert(id, op);
        }
    };

    // Binary arithmetic
    add("+", IntrinsicOp::Binary(BinOp::Add));
    add("-", IntrinsicOp::Binary(BinOp::Sub));
    add("*", IntrinsicOp::Binary(BinOp::Mul));
    add("/", IntrinsicOp::Binary(BinOp::Div));
    // `rem` uses truncated remainder, matching BinOp::Rem / Instruction::Rem.
    // `%` is Euclidean modulo (different for negative numbers) — not mapped.
    add("rem", IntrinsicOp::Binary(BinOp::Rem));

    // Comparisons
    add("=", IntrinsicOp::Compare(CmpOp::Eq));
    add("<", IntrinsicOp::Compare(CmpOp::Lt));
    add(">", IntrinsicOp::Compare(CmpOp::Gt));
    add("<=", IntrinsicOp::Compare(CmpOp::Le));
    add(">=", IntrinsicOp::Compare(CmpOp::Ge));

    // Unary
    // `-` with 1 arg is handled as a special case in try_lower_intrinsic.
    add("not", IntrinsicOp::Unary(UnaryOp::Not));

    map
}

/// Primitives guaranteed to return NaN-boxed immediates.
///
/// Every name here has been verified: on success, the primitive returns
/// `Value::int(...)`, `Value::float(...)`, `Value::bool(...)`, or
/// `Value::keyword(...)` — all NaN-boxed immediates that `RegionExit`
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
    "tuple?",
    "table?",
    "struct?",
    "buffer?",
    "bytes?",
    "blob?",
    "pointer?",
    "fiber?",
    "closure?",
    "jit?",
    "pure?",
    "coro?",
    "box?",
    // Collection predicates → bool
    "empty?",
    "has-key?",
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
];

/// Build the set of primitive SymbolIds known to return immediates.
///
/// Used by escape analysis to accept `(let (...) (length x))` and
/// similar patterns where the body result is a call to one of these
/// primitives.
pub fn build_immediate_primitives(symbols: &SymbolTable) -> FxHashSet<SymbolId> {
    let mut set = FxHashSet::default();
    for &name in IMMEDIATE_PRIMITIVES {
        if let Some(id) = symbols.get(name) {
            set.insert(id);
        }
    }
    set
}
