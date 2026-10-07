// audited: 2026-10-06
//! What a condition or a `(type-of x)` dispatch evaluates to, read off the inferred types.
//!
//! docs/impl/graft.md

use super::super::guard::{intrinsic_predicate_type, named_predicate_type};
use super::super::infer::unwrap_anf_let;
use super::super::unwrap_callee_binding;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};
use crate::hir::pattern::{HirPattern, PatternLiteral};
use crate::hir::types::{TyId, TypeInterner};
use crate::value::SymbolId;
use std::collections::HashMap;

/// What a condition is known to evaluate to.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) enum Decided {
    True,
    False,
    /// Free of effects, but its truth is not decided.
    Unknown,
    /// Outside the grammar the fold may delete: it may have an effect.
    Opaque,
}

impl Decided {
    fn of(b: bool) -> Self {
        if b {
            Decided::True
        } else {
            Decided::False
        }
    }

    fn not(self) -> Self {
        match self {
            Decided::True => Decided::False,
            Decided::False => Decided::True,
            other => other,
        }
    }
}

/// The `type-of` keywords a known type answers, one bit each. A keyword the
/// lattice has no type for is never among them.
const KEYWORDS: [&str; 17] = [
    "integer", "float", "boolean", "nil", "string", "@string", "keyword", "symbol", "list",
    "bytes", "@bytes", "array", "@array", "struct", "@struct", "set", "@set",
];

fn keyword_bit(name: &str) -> u32 {
    let name = name.strip_prefix(':').unwrap_or(name);
    KEYWORDS
        .iter()
        .position(|k| *k == name)
        .map_or(0, |i| 1 << i)
}

/// The keywords `(type-of v)` may answer for a value of type `t`, when the
/// lattice pins them.
fn keywords_of(t: TyId) -> Option<u32> {
    let names: &[&str] = match t {
        TypeInterner::INT => &["integer"],
        TypeInterner::FLOAT => &["float"],
        TypeInterner::NUMBER => &["integer", "float"],
        TypeInterner::BOOL => &["boolean"],
        TypeInterner::NIL => &["nil"],
        TypeInterner::STRING => &["string"],
        TypeInterner::MUTABLE_STRING => &["@string"],
        TypeInterner::KEYWORD => &["keyword"],
        TypeInterner::SYMBOL => &["symbol"],
        TypeInterner::EMPTY_LIST | TypeInterner::PAIR => &["list"],
        TypeInterner::BYTES => &["bytes"],
        TypeInterner::MUTABLE_BYTES => &["@bytes"],
        TypeInterner::ARRAY => &["array"],
        TypeInterner::MUTABLE_ARRAY => &["@array"],
        TypeInterner::STRUCT => &["struct"],
        TypeInterner::MUTABLE_STRUCT => &["@struct"],
        TypeInterner::SET => &["set"],
        TypeInterner::MUTABLE_SET => &["@set"],
        _ => return None,
    };
    Some(names.iter().fold(0, |acc, n| acc | keyword_bit(n)))
}

/// What a `type-of` arm's pattern matches.
enum ArmKeywords {
    /// A keyword, or an `or` of keywords.
    Some(u32),
    /// `_`: every keyword.
    All,
    /// A variable, which binds the keyword, or a pattern nothing here reads.
    Other,
}

fn arm_keywords(p: &HirPattern) -> ArmKeywords {
    fn keyword(p: &HirPattern) -> Option<u32> {
        match p {
            HirPattern::Literal(PatternLiteral::Keyword(k)) => Some(keyword_bit(k)),
            _ => None,
        }
    }
    match p {
        HirPattern::Wildcard => ArmKeywords::All,
        HirPattern::Or(alts) => alts
            .iter()
            .map(keyword)
            .try_fold(0, |acc, k| Some(acc | k?))
            .map_or(ArmKeywords::Other, ArmKeywords::Some),
        _ => keyword(p).map_or(ArmKeywords::Other, ArmKeywords::Some),
    }
}

/// Reads decisions off one round's inferred types.
pub(super) struct Decider<'a> {
    types: &'a HashMap<HirId, TyId>,
    arena: &'a BindingArena,
    interner: TypeInterner,
}

impl<'a> Decider<'a> {
    pub(super) fn new(types: &'a HashMap<HirId, TyId>, arena: &'a BindingArena) -> Self {
        Decider {
            types,
            arena,
            interner: TypeInterner::new(),
        }
    }

    /// What `cond` evaluates to, as a condition.
    pub(super) fn condition(&self, cond: &Hir) -> Decided {
        self.cond(cond, &mut Vec::new())
    }

    /// The one arm of a `(match (type-of x) …)` certain to run: every arm
    /// before it cannot match the operand's type, and it matches every value
    /// of that type.
    pub(super) fn match_arm(
        &self,
        value: &Hir,
        arms: &[(HirPattern, Option<Hir>, Hir)],
    ) -> Option<usize> {
        let remaining = self.subject_keywords(value)?;
        for (i, (pat, guard, _)) in arms.iter().enumerate() {
            let p = match arm_keywords(pat) {
                ArmKeywords::Some(p) => p,
                ArmKeywords::All => u32::MAX,
                ArmKeywords::Other => return None,
            };
            if p & remaining == 0 {
                continue;
            }
            if guard.is_some() || remaining & !p != 0 {
                return None;
            }
            return Some(i);
        }
        None
    }

    fn cond(&self, h: &Hir, env: &mut Vec<(Binding, Decided)>) -> Decided {
        match &h.kind {
            HirKind::Bool(b) => Decided::of(*b),
            HirKind::Nil => Decided::False,
            HirKind::Int(_)
            | HirKind::Float(_)
            | HirKind::String(_)
            | HirKind::Keyword(_)
            | HirKind::EmptyList => Decided::True,
            HirKind::Var(b) => env
                .iter()
                .rev()
                .find(|(x, _)| x == b)
                .map_or(Decided::Unknown, |(_, d)| *d),
            HirKind::DerefCell { cell } if matches!(cell.kind, HirKind::Var(_)) => Decided::Unknown,
            HirKind::Intrinsic {
                op: IntrinsicOp::Not,
                args,
            } if args.len() == 1 => self.cond(&args[0], env).not(),
            HirKind::Intrinsic { op, args } if args.len() == 1 && trivial(&args[0]) => {
                match intrinsic_predicate_type(*op) {
                    Some(t) => self.test(&args[0], t),
                    None => Decided::Opaque,
                }
            }
            HirKind::Call { func, args, .. }
                if args.len() == 1 && !args[0].spliced && trivial(&args[0].expr) =>
            {
                match self.named_predicate(func) {
                    Some(t) => self.test(&args[0].expr, t),
                    None => Decided::Opaque,
                }
            }
            HirKind::And(xs) => self.all(xs, env, Decided::False, Decided::True),
            HirKind::Or(xs) => self.all(xs, env, Decided::True, Decided::False),
            HirKind::Let { bindings, body } => {
                let depth = env.len();
                for (b, init) in bindings {
                    let d = self.cond(init, env);
                    if d == Decided::Opaque {
                        env.truncate(depth);
                        return Decided::Opaque;
                    }
                    env.push((*b, d));
                }
                let d = self.cond(body, env);
                env.truncate(depth);
                d
            }
            HirKind::If {
                cond,
                then_branch,
                else_branch,
            } => {
                let (c, t, e) = (
                    self.cond(cond, env),
                    self.cond(then_branch, env),
                    self.cond(else_branch, env),
                );
                if [c, t, e].contains(&Decided::Opaque) {
                    return Decided::Opaque;
                }
                match c {
                    Decided::True => t,
                    Decided::False => e,
                    _ if t == e => t,
                    _ => Decided::Unknown,
                }
            }
            HirKind::Match { value, arms } => self.match_value(value, arms, env),
            HirKind::Begin(xs) if xs.len() == 1 => self.cond(&xs[0], env),
            _ => Decided::Opaque,
        }
    }

    /// An `and` (`short` False, `whole` True) or an `or` (the other way round):
    /// one operand deciding `short` decides it, provided every operand is free
    /// of effects, since the fold deletes the whole condition.
    fn all(
        &self,
        xs: &[Hir],
        env: &mut Vec<(Binding, Decided)>,
        short: Decided,
        whole: Decided,
    ) -> Decided {
        let ds: Vec<Decided> = xs.iter().map(|x| self.cond(x, env)).collect();
        if ds.contains(&Decided::Opaque) {
            Decided::Opaque
        } else if ds.contains(&short) {
            short
        } else if ds.iter().all(|d| *d == whole) {
            whole
        } else {
            Decided::Unknown
        }
    }

    /// A `(match (type-of x) …)` read as a value: the truth every arm that can
    /// run answers, when they agree.
    fn match_value(
        &self,
        value: &Hir,
        arms: &[(HirPattern, Option<Hir>, Hir)],
        env: &mut Vec<(Binding, Decided)>,
    ) -> Decided {
        if !self.is_typeof(value) {
            return Decided::Opaque;
        }
        let known = self.subject_keywords(value);
        let mut remaining = known.unwrap_or(u32::MAX);
        let mut answer: Option<Decided> = None;
        for (pat, guard, body) in arms {
            let p = match arm_keywords(pat) {
                ArmKeywords::Some(p) => p,
                ArmKeywords::All => u32::MAX,
                ArmKeywords::Other => return Decided::Opaque,
            };
            if guard.is_some() {
                return Decided::Opaque;
            }
            if known.is_some() && p & remaining == 0 {
                continue;
            }
            let d = self.cond(body, env);
            if d == Decided::Opaque {
                return Decided::Opaque;
            }
            answer = Some(match answer {
                None => d,
                Some(a) if a == d => a,
                Some(_) => Decided::Unknown,
            });
            if remaining & !p == 0 {
                return answer.unwrap_or(Decided::Unknown);
            }
            remaining &= !p;
        }
        // No arm is certain to match, so the match can raise.
        Decided::Opaque
    }

    /// Is `value` a `(type-of x)` of an operand free of effects?
    fn is_typeof(&self, value: &Hir) -> bool {
        self.typeof_operand(value).is_some()
    }

    /// The keywords the scrutinee `(type-of x)` may answer, from `x`'s type.
    fn subject_keywords(&self, value: &Hir) -> Option<u32> {
        let operand = self.typeof_operand(value)?;
        keywords_of(self.type_of(operand))
    }

    fn typeof_operand<'h>(&self, value: &'h Hir) -> Option<&'h Hir> {
        let operand = match &unwrap_anf_let(value).kind {
            HirKind::Intrinsic {
                op: IntrinsicOp::TypeOf,
                args,
            } if args.len() == 1 => &args[0],
            HirKind::Call { func, args, .. } if args.len() == 1 && !args[0].spliced => {
                let callee = self.arena.get(unwrap_callee_binding(func)?);
                let is_type_of =
                    callee.name == SymbolId::of("type-of") || callee.name == SymbolId::of("type");
                if !(is_type_of && callee.is_primitive && callee.is_immutable && !callee.is_mutated)
                {
                    return None;
                }
                &args[0].expr
            }
            _ => return None,
        };
        trivial(operand).then_some(operand)
    }

    /// A predicate named by a binding the runtime supplies.
    fn named_predicate(&self, func: &Hir) -> Option<TyId> {
        let callee = self.arena.get(unwrap_callee_binding(func)?);
        if !callee.is_primitive || !callee.is_immutable || callee.is_mutated {
            return None;
        }
        named_predicate_type(callee.name)
    }

    /// A type test of `operand` against `tested`.
    fn test(&self, operand: &Hir, tested: TyId) -> Decided {
        let t = self.type_of(operand);
        if t == TypeInterner::TOP || t == TypeInterner::BOTTOM {
            Decided::Unknown
        } else if self.interner.subtype(t, tested) {
            Decided::True
        } else if self.interner.meet(t, tested) == TypeInterner::BOTTOM {
            Decided::False
        } else {
            Decided::Unknown
        }
    }

    fn type_of(&self, h: &Hir) -> TyId {
        self.types.get(&h.id).copied().unwrap_or(TypeInterner::TOP)
    }
}

/// A literal or a read of a binding: an operand whose evaluation the fold may
/// delete.
fn trivial(h: &Hir) -> bool {
    match &h.kind {
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_) => true,
        HirKind::DerefCell { cell } => matches!(cell.kind, HirKind::Var(_)),
        _ => false,
    }
}
