//! Container-dispatch wrapper monomorphization — collapse `(match (type-of coll)
//! …)` through the call boundary when the container type is statically proven.
//!
//! ## The shape and the leak it removes
//!
//! The collection-mutation wrappers (`push`, `put`, and the remove/add wrappers)
//! are type-dispatch closures: `(match (type-of coll) :array (%put-array …) :@array
//! (%put-array-mut …) … _ (dynamic …))`, each arm routing the SAME container `coll`
//! to a monomorphic `%`-op. The container is referenced in every arm, so the region
//! solver places its single owned-arg release in the textually-last arm — a block the
//! executed path never reaches, so the moved-in container argument's region is never
//! reclaimed (one leaked region per call; `memory.md` § F1b, the dispatch-wrapper
//! passthrough leak). A hand-collapsed single-arm wrapper does NOT leak: with one arm
//! the release lands on the executed path.
//!
//! ## What this pass does
//!
//! At a call `(put s :x j)` whose container argument `s` has a statically-proven
//! concrete container type (from the inference `hir_types`), the runtime dispatch is
//! dead code for every arm but the one that type selects. This pass rewrites the call
//! to a direct call to that arm's monomorphic op — `(%put-struct-mut s :x j)` — so the
//! multi-arm dispatch, and the container over-keep it strands, cease to exist. Where
//! the container type is genuinely dynamic (a parameter joined to Top across disjoint
//! callers), no arm is statically selected and the wrapper call is left intact (the
//! dynamic case is the branch-compensation fallback's, `regions::compensate`).
//!
//! This is the function-boundary generalization of the `each`-macro dead-arm prune
//! (`prune.rs`): there the dispatch is inlined by macro expansion and the dead arms
//! removed in place; here the dispatch lives behind a call, and the whole call
//! collapses to the live arm. It is behavior-preserving — the rewritten op is exactly
//! the arm the proven type would run, and its operand contract (`contract.rs`) is
//! discharged by the same proof that selected it, so it is checked like any other
//! call-position `%`-op immediately after.
//!
//! ## Recognition is structural, not a name allowlist
//!
//! A wrapper is any function whose body reaches a `(match (type-of param0) …)` whose
//! container arms are each a single call to a **primitive** op over the wrapper's
//! parameters (a fixed param, or the first element of a `& rest` — the `put` 2-vs-3
//! arity shape). So push/put and any future remove/add wrapper of the same shape are
//! covered without enumerating names. An arm operand that is neither a fixed parameter
//! nor the recognized rest element, or a call arity that does not match the arm's
//! operand count 1:1 in order, disqualifies the wrapper (left dynamic — never
//! mis-rewritten).

use super::infer::{pattern_type_keyword, typeof_subject_binding, unwrap_anf_let, var_of};
use super::unwrap_callee_binding;
use crate::hir::arena::BindingArena;
use crate::hir::binding::Binding;
use crate::hir::expr::{Hir, HirId, HirKind, IntrinsicOp};
use crate::hir::types::TyId;
use std::collections::HashMap;

/// One container arm of a recognized dispatch wrapper: the concrete container type
/// it selects on, the monomorphic op it routes to, and the positional map from the
/// op's operands to the wrapper's logical arguments (a fixed-param index, or the
/// rest-first index == `params.len()`).
struct Arm {
    ty: TyId,
    native: Binding,
    arg_src: Vec<usize>,
}

/// A recognized container-dispatch wrapper: its fixed params and its container arms.
/// `arity` is the logical argument count a call must have to map 1:1 onto an arm's
/// operands (fixed params, plus one for the `& rest` first element when present).
struct Wrapper {
    arity: usize,
    arms: Vec<Arm>,
}

/// Rewrite every container-dispatch wrapper call whose container argument's type is a
/// statically-proven concrete container to a direct call to the selected arm's
/// monomorphic op. Runs after the inference fixpoint (so `hir_types` is populated) and
/// before the intrinsic operand proofs (so the rewritten op is contract-checked).
pub(super) fn monomorphize_dispatch_wrappers(
    hir: &mut Hir,
    hir_types: &HashMap<HirId, TyId>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    typeof_aliases: &HashMap<Binding, Binding>,
) {
    let mut wrappers: HashMap<Binding, Wrapper> = HashMap::new();
    collect_wrappers(hir, arena, symbol_names, typeof_aliases, &mut wrappers);
    if wrappers.is_empty() {
        return;
    }
    rewrite(hir, hir_types, &wrappers);
}

/// Walk every `Let`/`Letrec`/`Define` lambda binding and record it as a dispatch
/// wrapper when its body reaches a container `(match (type-of param0) …)`.
fn collect_wrappers(
    hir: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    typeof_aliases: &HashMap<Binding, Binding>,
    out: &mut HashMap<Binding, Wrapper>,
) {
    let record = |b: Binding, value: &Hir, out: &mut HashMap<Binding, Wrapper>| {
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        if let HirKind::Lambda {
            params,
            rest_param,
            body,
            ..
        } = &value.kind
        {
            if let Some(w) = build_wrapper(
                params,
                *rest_param,
                body,
                arena,
                symbol_names,
                typeof_aliases,
            ) {
                out.insert(b, w);
            }
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, value) in bindings {
                record(*b, value, out);
            }
        }
        HirKind::Define { binding, value } => record(*binding, value, out),
        _ => {}
    }
    hir.for_each_child(|c| collect_wrappers(c, arena, symbol_names, typeof_aliases, out));
}

/// Build a `Wrapper` from a lambda's params/body when the body dispatches on
/// `(type-of param0)` with monomorphic container arms; `None` when the shape does
/// not qualify (left dynamic).
fn build_wrapper(
    params: &[Binding],
    rest_param: Option<Binding>,
    body: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    typeof_aliases: &HashMap<Binding, Binding>,
) -> Option<Wrapper> {
    let param0 = *params.first()?;
    // The local bound to `(first rest)`, if any — `put`'s 3-arg value operand. Maps to
    // logical index `params.len()` (right after the fixed params).
    let rest_first = rest_param.and_then(|rp| find_rest_first_local(body, rp, arena, symbol_names));
    let arms = find_arms(
        body,
        param0,
        params,
        rest_first,
        arena,
        symbol_names,
        typeof_aliases,
    )?;
    if arms.is_empty() {
        return None;
    }
    // Every arm must consume the same, contiguous, in-order argument list (0..n) so a
    // call with exactly `n` args maps 1:1 onto the operands. Derive `n` from the arms
    // and verify each is the identity map (0,1,…,n-1).
    let arity = arms.iter().map(|a| a.arg_src.len()).max().unwrap_or(0);
    for a in &arms {
        if a.arg_src.len() != arity || a.arg_src.iter().enumerate().any(|(i, &s)| i != s) {
            return None;
        }
    }
    Some(Wrapper { arity, arms })
}

/// Find the `(match (type-of param0) …)` within a wrapper body — searching through the
/// arity guards (`put`'s `(if (empty? rest) … (let [val (first rest)] <match>))`) — and
/// build its container arms (owned, so no borrow escapes the traversal). `None` when no
/// such dispatch exists OR a container arm is not a clean primitive call over the
/// wrapper's args (disqualifying: a partial rewrite could mis-map operands).
#[allow(clippy::too_many_arguments)]
fn find_arms(
    body: &Hir,
    param0: Binding,
    params: &[Binding],
    rest_first: Option<Binding>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    typeof_aliases: &HashMap<Binding, Binding>,
) -> Option<Vec<Arm>> {
    if let HirKind::Match { value, arms } = &body.kind {
        if typeof_subject_binding(value, arena, symbol_names, typeof_aliases) == Some(param0) {
            let mut out = Vec::new();
            for (pat, _guard, arm_body) in arms {
                let Some(ty) = pattern_type_keyword(pat) else {
                    continue; // wildcard / non-container arm (the `_ (dynamic …)` fallback)
                };
                let (native, arg_src) = extract_mono_arm(arm_body, params, rest_first, arena)?;
                out.push(Arm {
                    ty,
                    native,
                    arg_src,
                });
            }
            return Some(out);
        }
    }
    let mut found = None;
    body.for_each_child(|c| {
        if found.is_none() {
            found = find_arms(
                c,
                param0,
                params,
                rest_first,
                arena,
                symbol_names,
                typeof_aliases,
            );
        }
    });
    found
}

/// The local binding bound to `(first <rest_param>)` within `body`, if any.
fn find_rest_first_local(
    body: &Hir,
    rest_param: Binding,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Option<Binding> {
    fn is_first_of(
        init: &Hir,
        rest: Binding,
        arena: &BindingArena,
        names: &HashMap<u32, String>,
    ) -> bool {
        let inner = unwrap_anf_let(init);
        match &inner.kind {
            HirKind::Intrinsic {
                op: IntrinsicOp::First,
                args,
            } => args.len() == 1 && var_of(&args[0]) == Some(rest),
            HirKind::Call { func, args, .. } if args.len() == 1 => {
                unwrap_callee_binding(func)
                    .and_then(|b| names.get(&arena.get(b).name.0))
                    .map(String::as_str)
                    == Some("first")
                    && var_of(&args[0].expr) == Some(rest)
            }
            _ => false,
        }
    }
    let mut found: Option<Binding> = None;
    fn go(
        h: &Hir,
        rest: Binding,
        arena: &BindingArena,
        names: &HashMap<u32, String>,
        found: &mut Option<Binding>,
    ) {
        if let HirKind::Let { bindings, .. } = &h.kind {
            for (b, init) in bindings {
                if found.is_none() && is_first_of(init, rest, arena, names) {
                    *found = Some(*b);
                }
            }
        }
        h.for_each_child(|c| go(c, rest, arena, names, found));
    }
    go(body, rest_param, arena, symbol_names, &mut found);
    found
}

/// From a container arm's body, extract the monomorphic op's binding and the source
/// index of each of its operands (a fixed-param index, or `params.len()` for the
/// rest-first local). `None` when the arm is not a single primitive call over exactly
/// those bindings.
fn extract_mono_arm(
    arm_body: &Hir,
    params: &[Binding],
    rest_first: Option<Binding>,
    arena: &BindingArena,
) -> Option<(Binding, Vec<usize>)> {
    // Peel the ANF `(let [_ CALL] (return _))` / `(return CALL)` wrappers around the arm.
    let call = peel_to_call(arm_body)?;
    let HirKind::Call { func, args, .. } = &call.kind else {
        return None;
    };
    let native = unwrap_callee_binding(func)?;
    if !arena.get(native).is_primitive {
        return None;
    }
    let mut arg_src = Vec::with_capacity(args.len());
    for a in args {
        let b = var_of(&a.expr)?;
        let idx = params
            .iter()
            .position(|&p| p == b)
            .or_else(|| (rest_first == Some(b)).then_some(params.len()))?;
        arg_src.push(idx);
    }
    Some((native, arg_src))
}

/// Unwrap the ANF result-naming / `Return` around an arm body to the underlying call
/// (`(let [t CALL] (return t))` / `(return CALL)` / `CALL`).
fn peel_to_call(h: &Hir) -> Option<&Hir> {
    let mut cur = h;
    loop {
        match &cur.kind {
            HirKind::Return { value } => cur = value,
            HirKind::Let { bindings, body } => {
                // ANF `(let [t CALL] t-or-(return t))`: follow the body back to the
                // single bound init.
                let b = var_of(unwrap_return(body))?;
                let (_, init) = bindings.iter().find(|(bb, _)| *bb == b)?;
                cur = init;
            }
            HirKind::Call { .. } => return Some(cur),
            _ => return None,
        }
    }
}

fn unwrap_return(h: &Hir) -> &Hir {
    match &h.kind {
        HirKind::Return { value } => value,
        _ => h,
    }
}

/// Walk the tree, rewriting each qualifying wrapper call to its selected arm's op.
fn rewrite(hir: &mut Hir, hir_types: &HashMap<HirId, TyId>, wrappers: &HashMap<Binding, Wrapper>) {
    hir.for_each_child_mut(|c| rewrite(c, hir_types, wrappers));

    let HirKind::Call {
        func,
        args,
        is_tail,
    } = &hir.kind
    else {
        return;
    };
    let Some(wrapper_b) = unwrap_callee_binding(func) else {
        return;
    };
    let Some(w) = wrappers.get(&wrapper_b) else {
        return;
    };
    // Only a call whose argument count maps 1:1 onto the arms' operands (no splices).
    if args.len() != w.arity || args.iter().any(|a| a.spliced) {
        return;
    }
    let Some(container) = args.first() else {
        return;
    };
    let Some(&cty) = hir_types.get(&arg_type_id(&container.expr)) else {
        return;
    };
    let Some(arm) = w.arms.iter().find(|a| a.ty == cty) else {
        return; // no arm selects this (dynamic, or a non-container type) — leave intact
    };

    // Build the monomorphic call: the arm's op over this call's args, in order. Fresh
    // nodes (fresh HirIds) so the later region walk's per-id side tables do not collide
    // with the replaced wrapper call's id. The arg nodes are reused as-is, keeping their
    // inferred types for the operand-proof check that follows.
    let is_tail = *is_tail;
    let native = arm.native;
    let HirKind::Call { args, .. } = std::mem::replace(&mut hir.kind, HirKind::Error) else {
        unreachable!("just matched Call");
    };
    let span = hir.span.clone();
    let signal = hir.signal;
    let new_func = Box::new(Hir::silent(HirKind::Var(native), span.clone()));
    *hir = Hir::new(
        HirKind::Call {
            func: new_func,
            args,
            is_tail,
        },
        span,
        signal,
    );
}

/// The HirId whose inferred type describes the container argument — the arg node
/// itself, or the value an ANF `(let [t EXPR] t)` names.
fn arg_type_id(arg: &Hir) -> HirId {
    unwrap_anf_let(arg).id
}
