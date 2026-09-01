//! Guard-fact extraction: what a condition proves about bindings.
//!
//! One recognizer serves both consumers of guard knowledge:
//!
//! - `infer.rs` narrows **types** — inside an `If`'s branches, and across the
//!   rest of a `Begin` after a *diverging* guard statement (the stdlib wrapper
//!   shape: `(when (%not (number? b)) (error …))` proves `b : Number` on the
//!   fall-through path, because every path where it is not has diverged).
//! - `contract.rs` tracks **nonzero** facts for the `%div`/`%rem`/`%mod`
//!   divisor obligation, with the same branch/fall-through interpretation.
//!
//! A condition yields two fact sets: what holds when it is truthy and what
//! holds when it is falsy. `(%not P)` swaps them; `and` accumulates the truthy
//! side, `or` the falsy side (De Morgan's sound halves — the other halves are
//! disjunctions the flat fact sets cannot express, so they stay empty).

use super::*;
use crate::value::SymbolId;

/// One thing a condition proves about one binding.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Fact {
    /// The binding's value satisfies this type (meet with the accumulated type).
    TypeIs(Binding, TyId),
    /// The binding's value is a number ≠ 0 (the div-family divisor obligation).
    Nonzero(Binding),
}

/// The facts a condition proves, split by its truth value.
#[derive(Debug, Default)]
pub(super) struct CondFacts {
    pub when_true: Vec<Fact>,
    pub when_false: Vec<Fact>,
}

impl CondFacts {
    fn none() -> Self {
        Self::default()
    }

    fn negate(self) -> Self {
        CondFacts {
            when_true: self.when_false,
            when_false: self.when_true,
        }
    }
}

/// The `Var`/`DerefCell{Var}` binding an expression reads, unwrapping the
/// ANF `(let [t EXPR] t)` naming if present.
fn subject(h: &Hir) -> Option<Binding> {
    var_of(unwrap_anf_let(h))
}

/// The type a 1-arg intrinsic predicate proves for its subject when truthy.
///
/// Only predicates whose truth pins a single lattice point appear: `%string?`
/// and `%bytes?` accept both mutabilities, `%array?`/`%struct?`/`%set?` both
/// mutabilities of their family, so their truth proves no single `TyId` and
/// they narrow nothing (the flat lattice has no unions).
fn intrinsic_predicate_type(op: IntrinsicOp) -> Option<TyId> {
    match op {
        IntrinsicOp::IsInt => Some(TypeInterner::INT),
        IntrinsicOp::IsFloat => Some(TypeInterner::FLOAT),
        IntrinsicOp::IsKeyword => Some(TypeInterner::KEYWORD),
        IntrinsicOp::IsSymbol => Some(TypeInterner::SYMBOL),
        IntrinsicOp::IsBool => Some(TypeInterner::BOOL),
        IntrinsicOp::IsNil => Some(TypeInterner::NIL),
        IntrinsicOp::IsEmpty => Some(TypeInterner::EMPTY_LIST),
        IntrinsicOp::IsPair => Some(TypeInterner::PAIR),
        _ => None,
    }
}

/// The type a *stdlib-named* predicate call proves when truthy. Recognition is
/// by callee name, the same authority the `match (type-of x)` narrowing uses:
/// these names are the language's stable predicate vocabulary.
fn named_predicate_type(sym: SymbolId) -> Option<TyId> {
    // Const patterns, not `symbols.name(sym)`: the question is identity, and
    // `SymbolId::of` is a `const fn` over the name's hash, so these compile to
    // integer compares against constants with no table in scope
    // (docs/impl/symbol.md § "Reading a name, and not reading one"). Several of
    // these predicates are defined in the stdlib rather than the primitive
    // tables, so nothing build-time could hand back their spelling anyway.
    const INT_P: SymbolId = SymbolId::of("int?");
    const INTEGER_P: SymbolId = SymbolId::of("integer?");
    const FLOAT_P: SymbolId = SymbolId::of("float?");
    const NUMBER_P: SymbolId = SymbolId::of("number?");
    const KEYWORD_P: SymbolId = SymbolId::of("keyword?");
    const SYMBOL_P: SymbolId = SymbolId::of("symbol?");
    const BOOL_P: SymbolId = SymbolId::of("bool?");
    const BOOLEAN_P: SymbolId = SymbolId::of("boolean?");
    const NIL_P: SymbolId = SymbolId::of("nil?");
    const PAIR_P: SymbolId = SymbolId::of("pair?");
    match sym {
        INT_P | INTEGER_P => Some(TypeInterner::INT),
        FLOAT_P => Some(TypeInterner::FLOAT),
        NUMBER_P => Some(TypeInterner::NUMBER),
        KEYWORD_P => Some(TypeInterner::KEYWORD),
        SYMBOL_P => Some(TypeInterner::SYMBOL),
        BOOL_P | BOOLEAN_P => Some(TypeInterner::BOOL),
        NIL_P => Some(TypeInterner::NIL),
        PAIR_P => Some(TypeInterner::PAIR),
        _ => None,
    }
}

/// Is this expression a literal zero (the value the div-family opcodes are
/// not total for)?
fn literal_zero(h: &Hir) -> bool {
    match &unwrap_anf_let(h).kind {
        HirKind::Int(n) => *n == 0,
        HirKind::Float(f) => *f == 0.0,
        _ => false,
    }
}

/// For a 2-arg comparison against a literal zero, the compared binding.
fn zero_comparison_subject(args: &[Hir]) -> Option<Binding> {
    if args.len() != 2 {
        return None;
    }
    if literal_zero(&args[1]) {
        subject(&args[0])
    } else if literal_zero(&args[0]) {
        subject(&args[1])
    } else {
        None
    }
}

/// Extract the facts `cond` proves, by truth value.
pub(super) fn cond_facts(cond: &Hir, arena: &BindingArena) -> CondFacts {
    let cond = unwrap_anf_let(cond);
    match &cond.kind {
        HirKind::Intrinsic { op, args } => match op {
            IntrinsicOp::Not if args.len() == 1 => cond_facts(&args[0], arena).negate(),
            // (%eq x 0): falsy ⇒ x ≠ 0. (%ne x 0): truthy ⇒ x ≠ 0.
            IntrinsicOp::Eq => match zero_comparison_subject(args) {
                Some(b) => CondFacts {
                    when_true: vec![],
                    when_false: vec![Fact::Nonzero(b)],
                },
                None => CondFacts::none(),
            },
            IntrinsicOp::Ne => match zero_comparison_subject(args) {
                Some(b) => CondFacts {
                    when_true: vec![Fact::Nonzero(b)],
                    when_false: vec![],
                },
                None => CondFacts::none(),
            },
            // (%gt x 0) truthy ⇒ x > 0 ⇒ nonzero; (%le x 0) falsy ⇒ x > 0.
            IntrinsicOp::Gt if args.len() == 2 && literal_zero(&args[1]) => {
                match subject(&args[0]) {
                    Some(b) => CondFacts {
                        when_true: vec![Fact::Nonzero(b)],
                        when_false: vec![],
                    },
                    None => CondFacts::none(),
                }
            }
            IntrinsicOp::Le if args.len() == 2 && literal_zero(&args[1]) => {
                match subject(&args[0]) {
                    Some(b) => CondFacts {
                        when_true: vec![],
                        when_false: vec![Fact::Nonzero(b)],
                    },
                    None => CondFacts::none(),
                }
            }
            _ if args.len() == 1 => match (intrinsic_predicate_type(*op), subject(&args[0])) {
                (Some(ty), Some(b)) => CondFacts {
                    when_true: vec![Fact::TypeIs(b, ty)],
                    when_false: vec![],
                },
                _ => CondFacts::none(),
            },
            _ => CondFacts::none(),
        },
        HirKind::Call { func, args, .. } => {
            let Some(callee) = unwrap_callee_binding(func) else {
                return CondFacts::none();
            };
            const NOT: SymbolId = SymbolId::of("not");
            const ZERO_P: SymbolId = SymbolId::of("zero?");
            const EQ: SymbolId = SymbolId::of("=");
            let callee_sym = arena.get(callee).name;
            match (callee_sym, args.len()) {
                (NOT, 1) => cond_facts(&args[0].expr, arena).negate(),
                (ZERO_P, 1) => match subject(&args[0].expr) {
                    Some(b) => CondFacts {
                        when_true: vec![],
                        when_false: vec![Fact::Nonzero(b)],
                    },
                    None => CondFacts::none(),
                },
                (EQ, 2) => {
                    let b = if literal_zero(&args[1].expr) {
                        subject(&args[0].expr)
                    } else if literal_zero(&args[0].expr) {
                        subject(&args[1].expr)
                    } else {
                        None
                    };
                    match b {
                        Some(b) => CondFacts {
                            when_true: vec![],
                            when_false: vec![Fact::Nonzero(b)],
                        },
                        None => CondFacts::none(),
                    }
                }
                (_, 1) => match (named_predicate_type(callee_sym), subject(&args[0].expr)) {
                    (Some(ty), Some(b)) => CondFacts {
                        when_true: vec![Fact::TypeIs(b, ty)],
                        when_false: vec![],
                    },
                    _ => CondFacts::none(),
                },
                _ => CondFacts::none(),
            }
        }
        // (and a b): truthy ⇒ every conjunct truthy. The falsy side is a
        // disjunction the flat fact set cannot express.
        HirKind::And(items) => CondFacts {
            when_true: items
                .iter()
                .flat_map(|i| cond_facts(i, arena).when_true)
                .collect(),
            when_false: vec![],
        },
        // (or a b): falsy ⇒ every disjunct falsy.
        HirKind::Or(items) => CondFacts {
            when_true: vec![],
            when_false: items
                .iter()
                .flat_map(|i| cond_facts(i, arena).when_false)
                .collect(),
        },
        _ => CondFacts::none(),
    }
}

/// Does this expression never fall through to the next statement — every path
/// through it raises or transfers control?
///
/// The recognized diverging leaf is `(emit :error …)` (what the prelude
/// `error` macro expands to); `break`/`recur` transfer control. This is what
/// makes a guard's fall-through narrowing sound: after
/// `(when (%not (number? b)) (error …))`, every continuation where `b` is not
/// a number has left the straight-line path.
pub(super) fn diverges(h: &Hir) -> bool {
    match &h.kind {
        HirKind::Emit { signal, .. } => signal.intersects(crate::value::SIG_ERROR),
        HirKind::Begin(xs) => xs.last().is_some_and(diverges),
        HirKind::Block { body, .. } => body.last().is_some_and(diverges),
        HirKind::Let { body, .. } | HirKind::Letrec { body, .. } => diverges(body),
        HirKind::If {
            then_branch,
            else_branch,
            ..
        } => diverges(then_branch) && diverges(else_branch),
        HirKind::Break { .. } | HirKind::Recur { .. } => true,
        HirKind::Return { value } => diverges(value),
        _ => false,
    }
}

/// The facts that hold for the statements *after* `stmt` in a sequence: a
/// one-armed `If` whose taken branch diverges leaves the other branch's
/// entry facts standing on the fall-through.
pub(super) fn facts_after_statement(stmt: &Hir, arena: &BindingArena) -> Vec<Fact> {
    let HirKind::If {
        cond,
        then_branch,
        else_branch,
    } = &stmt.kind
    else {
        return vec![];
    };
    let then_div = diverges(then_branch);
    let else_div = diverges(else_branch);
    let facts = cond_facts(cond, arena);
    match (then_div, else_div) {
        (true, false) => facts.when_false,
        (false, true) => facts.when_true,
        _ => vec![],
    }
}
