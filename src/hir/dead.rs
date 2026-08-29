//! Dead binding elimination for unused `let`/`letrec` bindings.
//!
//! A binding nothing reads, whose initializer nothing can observe, is removable
//! along with the initializer. That deletes the dead call the initializer makes,
//! so it never reaches LIR and never mints a region.
//!
//! The design argument — why an effect-free proof rather than a silence check,
//! what the pass declines to touch, and why this altitude — lives in
//! [docs/impl/hir.md](../../docs/impl/hir.md) § "Dead binding elimination". The
//! comments here say only what a reader of this file would otherwise get wrong.

use rustc_hash::{FxHashMap, FxHashSet};

use super::arena::BindingArena;
use super::binding::Binding;
use super::defuse::DefUseBuilder;
use super::expr::{Hir, HirKind};
use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::symbol::SymbolTable;

/// Remove every `let`/`letrec` binding that has no use and an effect-free
/// initializer.
pub(crate) fn eliminate_dead_bindings(hir: &mut Hir, arena: &BindingArena, symbols: &SymbolTable) {
    // Phase 1 (read-only): who is read, and which callees are proven pure.
    let used: FxHashSet<Binding> = {
        let mut defuse = DefUseBuilder::new();
        defuse.walk(hir);
        defuse.uses.into_keys().collect()
    };
    let pure = pure_functions(hir, arena, symbols);

    // Phase 2 (mutating): drop the bindings.
    strip(hir, arena, symbols, &used, &pure);
}

/// Drop the eliminable bindings of every `Let`/`Letrec` in the tree.
///
/// A binding is retained *before* the walk descends, so a dropped initializer's
/// own subtree is never visited: whatever it bound went with it.
///
/// A node left with no bindings stays. Splicing its body in its place would
/// retire one scope region, but the empty scope is already correct, and every
/// downstream pass reads it as it reads any other `Let`.
fn strip(
    hir: &mut Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    used: &FxHashSet<Binding>,
    pure: &FxHashSet<Binding>,
) {
    if let HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } = &mut hir.kind {
        bindings.retain(|(b, init)| !eliminable(*b, init, arena, symbols, used, pure));
    }
    hir.for_each_child_mut(|c| strip(c, arena, symbols, used, pure));
}

/// Can this binding and its initializer both disappear?
fn eliminable(
    binding: Binding,
    init: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    used: &FxHashSet<Binding>,
    pure: &FxHashSet<Binding>,
) -> bool {
    if used.contains(&binding) {
        return false;
    }
    let inner = arena.get(binding);
    // `is_mutated` is the one flag that is not merely conservative: `assign`
    // records a *definition* in the def-use walk, so a written-and-never-read
    // binding reads as unused while an `Assign` node still names it. Removing
    // the binding would leave that node pointing at nothing.
    if inner.is_synthetic
        || inner.is_primitive
        || inner.is_mutated
        || inner.is_file_scope
        || inner.needs_capture()
    {
        return false;
    }
    if matches!(init.kind, HirKind::Lambda { .. }) {
        return false;
    }
    is_effect_free(init, arena, symbols, pure)
}

/// Can evaluating this expression be observed?
///
/// `false` is always the safe answer, so every kind the walk does not recognize
/// takes it. The recognized set is the value forms an initializer takes.
fn is_effect_free(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    pure: &FxHashSet<Binding>,
) -> bool {
    let all = |exprs: &[Hir]| {
        exprs
            .iter()
            .all(|e| is_effect_free(e, arena, symbols, pure))
    };
    match &hir.kind {
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Quote(_)
        | HirKind::QuoteConst(_)
        | HirKind::Var(_) => true,

        HirKind::Return { value } => is_effect_free(value, arena, symbols, pure),

        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            is_effect_free(cond, arena, symbols, pure)
                && is_effect_free(then_branch, arena, symbols, pure)
                && is_effect_free(else_branch, arena, symbols, pure)
        }

        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            clauses.iter().all(|(c, b)| {
                is_effect_free(c, arena, symbols, pure) && is_effect_free(b, arena, symbols, pure)
            }) && else_branch
                .as_ref()
                .is_none_or(|e| is_effect_free(e, arena, symbols, pure))
        }

        HirKind::Begin(exprs) | HirKind::And(exprs) | HirKind::Or(exprs) => all(exprs),

        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            bindings
                .iter()
                .all(|(_, init)| is_effect_free(init, arena, symbols, pure))
                && is_effect_free(body, arena, symbols, pure)
        }

        // The node's own signal already folds in the callee's signal and every
        // argument's, so a silent call node also proves the arguments silent and
        // proves a polymorphic callee got silent arguments. What it does NOT
        // prove is that the callee stores nothing — that is `callee_is_pure`.
        HirKind::Call { func, args, .. } => {
            hir.signal == Signal::silent()
                && callee_is_pure(func, arena, symbols, pure)
                && args
                    .iter()
                    .all(|a| is_effect_free(&a.expr, arena, symbols, pure))
        }

        // `routes_native_funnel` is the storing/removing/copying set, and the
        // analyzer routes a call-position use of one of those to its NativeFn
        // rather than to an opcode node. Testing it here anyway keeps the
        // mutation gate on the op, not on which node shape the analyzer chose.
        HirKind::Intrinsic { op, args } => {
            hir.signal == Signal::silent() && !op.routes_native_funnel() && all(args)
        }

        _ => false,
    }
}

/// Does calling this function position store nothing and reach nothing opaque?
///
/// A primitive answers from its own declaration. `RegionEffect::Immediate` and
/// `RegionEffect::Fresh` are the two that state no argument is stored anywhere
/// outliving the call — in-place mutation is such a store, so `Funnel`,
/// `Stores`, `Sends`, `Mixed`, and `PassThrough` all fail here. `moves_out`
/// marks the natives that remove an element from a container argument.
///
/// A user-defined function answers from `pure`, the fixpoint over lambda bodies.
/// Anything else — a parameter, a dynamic expression, a mutable or reassigned
/// binding — is opaque and fails.
fn callee_is_pure(
    func: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    pure: &FxHashSet<Binding>,
) -> bool {
    let HirKind::Var(binding) = &func.kind else {
        return false;
    };
    let inner = arena.get(*binding);
    if !inner.is_immutable || inner.is_mutated {
        return false;
    }
    if !inner.is_primitive {
        return pure.contains(binding);
    }
    let Some(name) = symbols.name(inner.name) else {
        return false;
    };
    let Some(def) = crate::primitives::registration::def_by_name(name) else {
        return false;
    };
    def.signal == Signal::silent()
        && matches!(def.effect, RegionEffect::Immediate | RegionEffect::Fresh)
        && !def.moves_out
}

/// The bindings whose lambda body is effect-free, to a fixpoint.
///
/// The fixpoint starts empty and grows, which is what keeps a self-recursive
/// function out of it: proving `r` pure would need `r` already proven. That is
/// the conservative direction — an unproven function's call survives.
fn pure_functions(hir: &Hir, arena: &BindingArena, symbols: &SymbolTable) -> FxHashSet<Binding> {
    let mut occurrences: FxHashMap<Binding, u32> = FxHashMap::default();
    count_binding_sites(hir, &mut occurrences);

    let mut pure: FxHashSet<Binding> = FxHashSet::default();
    loop {
        let mut grew = false;
        admit_pure_lambdas(hir, arena, symbols, &occurrences, &mut pure, &mut grew);
        if !grew {
            return pure;
        }
    }
}

/// Admit each not-yet-proven lambda binding whose body is effect-free under what
/// is proven so far. One round of the fixpoint.
fn admit_pure_lambdas(
    hir: &Hir,
    arena: &BindingArena,
    symbols: &SymbolTable,
    occurrences: &FxHashMap<Binding, u32>,
    pure: &mut FxHashSet<Binding>,
    grew: &mut bool,
) {
    let mut consider = |b: Binding, value: &Hir, pure: &mut FxHashSet<Binding>| {
        // Bound more than once means no single stable value, so the name proves
        // nothing about what a call site reaches.
        if occurrences.get(&b) != Some(&1) || pure.contains(&b) {
            return;
        }
        let inner = arena.get(b);
        if !inner.is_immutable || inner.is_mutated {
            return;
        }
        if let HirKind::Lambda { body, .. } = &value.kind {
            if is_effect_free(body, arena, symbols, pure) {
                pure.insert(b);
                *grew = true;
            }
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, value) in bindings {
                consider(*b, value, pure);
            }
        }
        HirKind::Define { binding, value } => consider(*binding, value, pure),
        _ => {}
    }
    hir.for_each_child(|c| admit_pure_lambdas(c, arena, symbols, occurrences, pure, grew));
}

/// How many `let`/`letrec`/`def` sites introduce each binding.
fn count_binding_sites(hir: &Hir, occurrences: &mut FxHashMap<Binding, u32>) {
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, _) in bindings {
                *occurrences.entry(*b).or_default() += 1;
            }
        }
        HirKind::Define { binding, .. } => *occurrences.entry(*binding).or_default() += 1,
        _ => {}
    }
    hir.for_each_child(|c| count_binding_sites(c, occurrences));
}

#[cfg(test)]
mod tests;
