//! Type-directed dead-arm pruning for `(match (type-of x) …)`.
//!
//! A `(type-of x)` dispatch routes on `x`'s runtime type: each keyword-literal
//! arm (`:array`, `(or :set :@set)`, `:fiber`, …) runs only when `x` has that
//! type. When `x`'s concrete type is *statically and soundly* known, every arm
//! whose keyword set cannot contain that type is provably unreachable. This pass
//! removes those arms from the HIR before region inference and escape analysis.
//!
//! ## Why this is the `each`-over-collection fix
//!
//! The `each` macro (`src/prelude.lisp`) is exactly this shape: it binds the
//! collection to a local and dispatches `(match (type-of seq) …)`, handing `seq`
//! to a per-type op (`->array`/`get`/`fiber/resume`) in each off-type arm. With
//! the arms intact, `seq` is *referenced in every arm*, so the region
//! solver computes `seq`'s last use — and thus its release point — at a use
//! inside the textually-last arm, a block the live path never reaches. The
//! collection's region is then never freed on the executed path: one leaked
//! object per `each` (pinned by the oracle's `each-array` probe vs the reclaiming
//! `each-manual` indexed loop; the branch-insensitive-escape imprecision class).
//! Removing the dead arms leaves `seq` referenced only in the live arm, so its
//! release lands on the executed path. The same pruning makes the escape analysis
//! precise for the ownership path — whatever a dead arm hands `seq` to, escape no
//! longer reads it — so the one transform serves both consumers. The
//! async scheduler's `each c in completions` is the same shape — io/wait declares
//! `RetType::Array`, so that dispatch prunes too — though that collection is not
//! itself an io-yield contributor (the oracle's io-yield probe is unchanged
//! whether or not that dispatch prunes).
//!
//! ## Soundness — the prune direction is UAF-critical
//!
//! Removing an arm is sound *only* if it can never run. So the concrete type is
//! taken from facts that cannot lie:
//!   - literals (`[…]`/`{…}`/`|…|`/numbers/strings/keywords/`()`), whose HIR
//!     constructor pins the `type-of` keyword exactly;
//!   - a call to a binding that is an actual **primitive** (`is_primitive`, set
//!     only by `bind_primitives`) with a concrete declared `RetType` — a
//!     user binding that merely *shadows* a primitive name is excluded, so its
//!     `RetType` is never read (that would mis-type the value → an unsound prune);
//!   - immutable, unmutated `Var` aliases of the above (followed to a fixpoint).
//!
//! Anything else leaves the scrutinee's type unknown and prunes nothing —
//! under-pruning is always safe (the arm survives), over-pruning is a UAF. The
//! keyword mapping (`keyword_of_rettype`) is fixed by `Value::type_name`
//! (`value/heap/objimpl.rs`, `value/repr/accessors.rs`) — the same strings every
//! `(type-of x)` dispatch matches on. A non-keyword arm (a wildcard `_`, a bound
//! variable, a guard-only arm) is never a "type-keyword arm" and is never pruned,
//! so a catch-all always survives; pruning that would empty a match is declined
//! (the match keeps its original arms and still raises `:match-error` for the
//! impossible runtime type).
//!
//! This runs **unconditionally** on every compile: it is a behavior-preserving
//! HIR simplification, not an intrinsic rewrite, and the downstream
//! region/escape analyses all see the pruned tree.
//!
//! It runs **before** `functionalize` (driven from `hir::regularize`): once
//! functionalize hoists a prebound/captured arm-local binding (the each macro's
//! `(def @cur seq)`) to the enclosing scope's cell layout, removing the arm would
//! orphan that hoisted cell and leave a reference the lowerer cannot resolve.
//! Pruning the dead arm first means functionalize never sees it.

use std::collections::HashMap;

use rustc_hash::{FxHashMap, FxHashSet};

use super::infer::{collect_typeof_aliases, typeof_subject_binding};
use super::unwrap_callee_binding;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirKind};
use crate::hir::pattern::{HirPattern, PatternLiteral};
use crate::primitives::def::RetType;

/// The keyword `type-of` returns for a value of this declared return type, or
/// `None` when the type is not a single concrete `type-of` category (`Unknown`,
/// `FirstArg`). Fixed by `Value::type_name`.
fn keyword_of_rettype(ret: RetType) -> Option<&'static str> {
    Some(match ret {
        RetType::Int => "integer",
        RetType::Float => "float",
        RetType::Bool => "boolean",
        RetType::String => "string",
        RetType::MutableString => "@string",
        RetType::Keyword => "keyword",
        RetType::Bytes => "bytes",
        RetType::MutableBytes => "@bytes",
        RetType::Array => "array",
        RetType::MutableArray => "@array",
        RetType::Struct => "struct",
        RetType::MutableStruct => "@struct",
        RetType::Set => "set",
        RetType::MutableSet => "@set",
        RetType::Fiber => "fiber",
        RetType::Unknown | RetType::FirstArg => return None,
    })
}

/// What a binding's single initializer tells us about its `type-of` keyword: a
/// resolved concrete keyword, or an alias to another binding whose keyword we
/// must follow. Built only for immutable, unmutated, singly-bound
/// `Let`/`Letrec`/`Define` bindings; everything else is absent (unknown).
#[derive(Clone, Copy)]
enum InitKw {
    Concrete(&'static str),
    Alias(Binding),
}

/// Map each statically-typeable binding to the concrete `type-of` keyword its
/// initializer proves — the sound `binding → keyword` proof this module's dead-arm
/// pruning runs on. It records every immutable, unmutated, singly-bound
/// `let`/`letrec`/`def` binding whose initializer resolves to a concrete type
/// (`classify_init`), following alias chains to a fixpoint (`resolve`).
///
/// Shared with map-chain fusion (`fuse.rs`), which reads it to prove a `map`'s
/// base collection is an immutable array (keyword `array`). The soundness bar is
/// identical and already enforced here: an over-broad classification would delete
/// a live match arm (a UAF), so a base fusion accepts on this map's word is proven
/// with the same rigor — never a value the compiler merely guesses is an array.
pub(super) fn concrete_init_keywords(
    hir: &Hir,
    arena: &BindingArena,
) -> FxHashMap<Binding, &'static str> {
    let mut init: FxHashMap<Binding, InitKw> = FxHashMap::default();
    let mut seen: FxHashSet<Binding> = FxHashSet::default();
    collect_inits(hir, arena, &mut init, &mut seen);

    // Resolve alias chains to a concrete keyword (depth-bounded; the alias graph
    // is over distinct binding ids, so a small cap both terminates any accidental
    // cycle and is far above real alias-chain depth).
    let mut concrete: FxHashMap<Binding, &'static str> = FxHashMap::default();
    for &b in init.keys() {
        if let Some(k) = resolve(b, &init, 0) {
            concrete.insert(b, k);
        }
    }
    concrete
}

/// Prune provably-dead arms of every `(match (type-of x) …)` whose scrutinee `x`
/// has a statically-known concrete type. See the module doc.
pub(crate) fn prune_typeof_match_arms(hir: &mut Hir, arena: &BindingArena) {
    // Phase 1 (read-only): the sound binding→keyword proof.
    let concrete = concrete_init_keywords(hir, arena);
    if concrete.is_empty() {
        return;
    }

    // A `(let [ta (type-of x)] (match ta …))` scrutinee resolves through this map
    // to `x`, so the aliased dispatch prunes like the inline `(match (type-of x)`.
    let mut typeof_aliases: HashMap<Binding, Binding> = HashMap::new();
    collect_typeof_aliases(hir, arena, &mut typeof_aliases);

    // Phase 2 (mutating): drop the dead arms.
    prune_node(hir, arena, &concrete, &typeof_aliases);
}

/// Walk every `Let`/`Letrec`/`Define` binding, recording its initializer keyword
/// when the binding is a stable single value (immutable, unmutated, bound once).
fn collect_inits(
    hir: &Hir,
    arena: &BindingArena,
    init: &mut FxHashMap<Binding, InitKw>,
    seen: &mut FxHashSet<Binding>,
) {
    let mut record = |b: Binding, value: &Hir, init: &mut FxHashMap<Binding, InitKw>| {
        // A binding bound more than once has no single stable value — drop it.
        if !seen.insert(b) {
            init.remove(&b);
            return;
        }
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        if let Some(k) = classify_init(value, arena) {
            init.insert(b, k);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, value) in bindings {
                record(*b, value, init);
            }
        }
        HirKind::Define { binding, value } => record(*binding, value, init),
        _ => {}
    }
    hir.for_each_child(|c| collect_inits(c, arena, init, seen));
}

/// The `type-of` keyword an initializer expression evaluates to (or an alias to
/// the binding it forwards), when soundly determinable. `None` ⇒ unknown.
fn classify_init(h: &Hir, arena: &BindingArena) -> Option<InitKw> {
    match &h.kind {
        HirKind::Int(_) => Some(InitKw::Concrete("integer")),
        HirKind::Float(_) => Some(InitKw::Concrete("float")),
        HirKind::Bool(_) => Some(InitKw::Concrete("boolean")),
        HirKind::String(_) => Some(InitKw::Concrete("string")),
        HirKind::Keyword(_) => Some(InitKw::Concrete("keyword")),
        HirKind::Nil => Some(InitKw::Concrete("nil")),
        HirKind::EmptyList => Some(InitKw::Concrete("list")),
        // Aliases: a bare var, or the ANF/region-transparent wrappers whose value
        // is their inner expression.
        HirKind::Var(b) => Some(InitKw::Alias(*b)),
        HirKind::MakeCell { value } | HirKind::Return { value } => classify_init(value, arena),
        // A call to a genuine primitive with a concrete declared return type. The
        // `is_primitive` gate is what makes reading the name's `RetType` sound — a
        // user binding shadowing the name is excluded (its `RetType` would be a
        // lie). This is how a collection literal (`[…]` → `(array …)`,
        // `RetType::Array`) and `io/wait`/`->array` (also `RetType::Array`) get a
        // concrete keyword.
        HirKind::Call { func, .. } => {
            let callee = unwrap_callee_binding(func)?;
            let bi = arena.get(callee);
            if !bi.is_primitive || !bi.is_immutable || bi.is_mutated {
                return None;
            }
            let def = crate::primitives::registration::def_by_symbol(bi.name)?;
            keyword_of_rettype(def.ret).map(InitKw::Concrete)
        }
        _ => None,
    }
}

/// Follow alias edges to a concrete keyword, depth-bounded.
fn resolve(b: Binding, init: &FxHashMap<Binding, InitKw>, depth: u32) -> Option<&'static str> {
    if depth > 32 {
        return None;
    }
    match init.get(&b)? {
        InitKw::Concrete(k) => Some(k),
        InitKw::Alias(a) => resolve(*a, init, depth + 1),
    }
}

/// At each `(match (type-of x) …)` whose `x` resolves to a concrete keyword,
/// drop the arms that keyword cannot reach.
fn prune_node(
    hir: &mut Hir,
    arena: &BindingArena,
    concrete: &FxHashMap<Binding, &'static str>,
    typeof_aliases: &HashMap<Binding, Binding>,
) {
    if let HirKind::Match { value, arms } = &mut hir.kind {
        if let Some(subj) = typeof_subject_binding(value, arena, typeof_aliases) {
            if let Some(&k) = concrete.get(&subj) {
                let dead = arms.iter().filter(|(p, _, _)| arm_is_dead(p, k)).count();
                // Prune only when it removes some — but not all — arms: an
                // all-dead match (the scrutinee matches no arm) is left intact so
                // it still raises `:match-error` exactly as before.
                if dead > 0 && dead < arms.len() {
                    arms.retain(|(p, _, _)| !arm_is_dead(p, k));
                }
            }
        }
    }
    hir.for_each_child_mut(|c| prune_node(c, arena, concrete, typeof_aliases));
}

/// Is this arm a *type-keyword* arm whose keyword set excludes `k`? Such an arm
/// can never match a value of type `k`, so it is dead. A non-keyword arm
/// (wildcard, variable, structural, or an `or` mixing in non-keyword patterns)
/// is not a type-keyword arm and is never reported dead. A guard is irrelevant:
/// a non-matching *pattern* fails before any guard runs.
fn arm_is_dead(pat: &HirPattern, k: &str) -> bool {
    match arm_keywords(pat) {
        Some(kws) => !kws.contains(&k),
        None => false,
    }
}

/// The keyword set an arm pattern matches, when the pattern is a keyword literal
/// or an `or` of keyword literals; `None` otherwise.
fn arm_keywords(pat: &HirPattern) -> Option<Vec<&str>> {
    fn kw(p: &HirPattern) -> Option<&str> {
        match p {
            HirPattern::Literal(PatternLiteral::Keyword(s)) => {
                Some(s.strip_prefix(':').unwrap_or(s))
            }
            _ => None,
        }
    }
    match pat {
        HirPattern::Literal(PatternLiteral::Keyword(_)) => Some(vec![kw(pat)?]),
        HirPattern::Or(alts) => alts.iter().map(kw).collect(),
        _ => None,
    }
}
