// audited: 2026-10-21
//! Guard-fact extraction: what a condition proves about bindings.
//!
//! docs/intrinsics.md
//! docs/impl/typeinfer.md
//! docs/impl/graft.md
//!
//! One recognizer serves every consumer of guard knowledge:
//!
//! - `infer.rs` narrows types inside a branch, and across the rest of a
//!   sequence after a guard statement whose taken branch diverges. After
//!   `(when (%not (number? b)) (error …))`, `b` is a Number, because every
//!   path where it is not has diverged.
//! - `contract.rs` tracks nonzero facts for the `%div`/`%rem`/`%mod` divisor
//!   obligation, with the same reading of branches and fall-through.
//! - The fold reads the predicate types to decide a type test.
//!
//! A condition yields two fact sets: what holds when it is truthy and what
//! holds when it is falsy. `(%not P)` swaps them; `and` accumulates the truthy
//! side, and `or` the falsy side. The other halves are disjunctions the flat
//! fact sets cannot express, so they stay empty.

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
/// Only predicates whose truth pins a single lattice point appear. `%string?`,
/// `%bytes?`, `%array?`, `%struct?` and `%set?` accept both mutabilities, and
/// the lattice has no type for the pair, so they narrow nothing.
pub(super) fn intrinsic_predicate_type(op: IntrinsicOp) -> Option<TyId> {
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
pub(super) fn named_predicate_type(sym: SymbolId) -> Option<TyId> {
    // Const patterns, not `symbols.name(sym)`: the question is identity, and
    // `SymbolId::of` is a `const fn` over the name's hash, so these compile to
    // integer compares against constants with no table in scope
    // (docs/impl/symbol.md). Several of these predicates are stdlib functions,
    // in no primitive table, so nothing built at compile time could hand back
    // their spelling.
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
    facts_in(cond, arena, &mut Vec::new())
}

/// [`cond_facts`], with `env` holding the `let` bindings the condition itself
/// introduced around the part being read: a condition that reads such a local
/// proves what the value it was bound to proves. A graft introduces them, so a
/// guard written with `not` or `integer?` still narrows once both are grafted.
fn facts_in<'h>(
    cond: &'h Hir,
    arena: &BindingArena,
    env: &mut Vec<(Binding, &'h Hir)>,
) -> CondFacts {
    let cond = unwrap_anf_let(cond);
    match &cond.kind {
        HirKind::Var(b) => match env.iter().rev().find(|(x, _)| x == b) {
            Some(&(_, init)) => facts_in(init, arena, env),
            None => CondFacts::none(),
        },
        HirKind::Let { bindings, body } => {
            let depth = env.len();
            env.extend(bindings.iter().map(|(b, init)| (*b, init)));
            let facts = facts_in(body, arena, env);
            env.truncate(depth);
            facts
        }
        HirKind::Match { value, arms } => typeof_match_facts(value, arms, arena),
        HirKind::Intrinsic { op, args } => match op {
            IntrinsicOp::Not if args.len() == 1 => facts_in(&args[0], arena, env).negate(),
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
            let inner = arena.get(callee);
            let callee_sym = inner.name;
            // `not` and a predicate prove a type only as the binding the
            // runtime supplies, never as a parameter that shares its name.
            let supplied = inner.is_primitive && inner.is_immutable && !inner.is_mutated;
            match (callee_sym, args.len()) {
                (NOT, 1) if supplied => facts_in(&args[0].expr, arena, env).negate(),
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
                (_, 1) if supplied => {
                    match (named_predicate_type(callee_sym), subject(&args[0].expr)) {
                        (Some(ty), Some(b)) => CondFacts {
                            when_true: vec![Fact::TypeIs(b, ty)],
                            when_false: vec![],
                        },
                        _ => CondFacts::none(),
                    }
                }
                _ => CondFacts::none(),
            }
        }
        // (and a b): truthy ⇒ every conjunct truthy. The falsy side is a
        // disjunction the flat fact set cannot express.
        HirKind::And(items) => {
            let mut when_true = Vec::new();
            for i in items {
                when_true.extend(facts_in(i, arena, env).when_true);
            }
            CondFacts {
                when_true,
                when_false: vec![],
            }
        }
        // (or a b): falsy ⇒ every disjunct falsy.
        HirKind::Or(items) => {
            let mut when_false = Vec::new();
            for i in items {
                when_false.extend(facts_in(i, arena, env).when_false);
            }
            CondFacts {
                when_true: vec![],
                when_false,
            }
        }
        _ => CondFacts::none(),
    }
}

/// What a `(match (type-of x) …)` whose arms answer `true` or `false` proves
/// when truthy: that `x` has one of the types whose arms answer `true`. That
/// set is a fact only when the lattice holds it exactly — `:integer` and
/// `:float` join to Number, while `:string` and `:@string` join to Top, which
/// proves nothing. An arm the grafted `number?` or `integer?` brings is this
/// shape.
fn typeof_match_facts(
    value: &Hir,
    arms: &[(crate::hir::pattern::HirPattern, Option<Hir>, Hir)],
    arena: &BindingArena,
) -> CondFacts {
    use crate::hir::pattern::HirPattern;
    let Some(subject) = typeof_call_subject(unwrap_anf_let(value), arena) else {
        return CondFacts::none();
    };
    let mut when_true: Vec<TyId> = Vec::new();
    for (pat, guard, body) in arms {
        let HirKind::Bool(answer) = body.kind else {
            return CondFacts::none();
        };
        if guard.is_some() {
            return CondFacts::none();
        }
        let alternatives: &[HirPattern] = match pat {
            HirPattern::Wildcard if answer => return CondFacts::none(),
            // Nothing after a wildcard can run.
            HirPattern::Wildcard => break,
            HirPattern::Or(alts) => alts,
            other => std::slice::from_ref(other),
        };
        for alt in alternatives {
            match (pattern_type_keyword(alt), answer) {
                (Some(ty), true) => when_true.push(ty),
                (Some(_), false) => {}
                // A keyword the lattice has no type for answers true, so the
                // truthy side is not a type.
                (None, true) => return CondFacts::none(),
                (None, false) if matches!(alt, HirPattern::Literal(_)) => {}
                (None, false) => return CondFacts::none(),
            }
        }
    }
    when_true.sort_by_key(|t| t.0);
    when_true.dedup();
    let proven = match when_true.as_slice() {
        [t] => *t,
        [TypeInterner::INT, TypeInterner::FLOAT] => TypeInterner::NUMBER,
        _ => return CondFacts::none(),
    };
    CondFacts {
        when_true: vec![Fact::TypeIs(subject, proven)],
        when_false: vec![],
    }
}

/// Does this expression never fall through to the next statement — every path
/// through it raises or transfers control?
///
/// The diverging leaf is an `emit` that carries `:error`; `break` and `recur`
/// transfer control. This makes a guard's fall-through narrowing sound: after
/// `(when (%not (number? b)) (error …))`, every continuation where `b` is not
/// a number has left the straight-line path.
///
/// A `let` runs its inits before its body, so a diverging init diverges the
/// form. The ANF lift names a raise that way: `(let [t (error …)] t)`.
/// A `block` that a `break` inside it names falls through to the statement
/// after it, whatever its tail does.
pub(super) fn diverges(h: &Hir) -> bool {
    match &h.kind {
        HirKind::Emit { signal, .. } => signal.intersects(crate::value::SIG_ERROR),
        HirKind::Begin(xs) => xs.last().is_some_and(diverges),
        HirKind::Block { block_id, body, .. } => {
            body.last().is_some_and(diverges) && !body.iter().any(|s| breaks_to(s, *block_id))
        }
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            bindings.iter().any(|(_, init)| diverges(init)) || diverges(body)
        }
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

/// Does a `break` inside `h` name the block `id`? A `break` never crosses a
/// function boundary, so a lambda's body is not searched.
fn breaks_to(h: &Hir, id: crate::hir::expr::BlockId) -> bool {
    match &h.kind {
        HirKind::Break { block_id, .. } if *block_id == id => true,
        HirKind::Lambda(_) => false,
        _ => {
            let mut found = false;
            h.for_each_child(|c| found = found || breaks_to(c, id));
            found
        }
    }
}

/// The facts that hold for the statements after `stmt` in a sequence: an `If`
/// with exactly one diverging branch leaves the other branch's entry facts
/// standing on the fall-through.
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
