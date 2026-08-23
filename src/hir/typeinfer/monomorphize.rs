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
//! reclaimed (one leaked region per call — the dispatch-wrapper passthrough leak,
//! pinned by the `native-tail-put-*` oracle controls). A hand-collapsed single-arm
//! wrapper does NOT leak: with one arm
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
//! dynamic case is the branch-compensation fallback's, `region::infer::compensate`).
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
use crate::hir::types::{TyId, TypeInterner};
use crate::value::SymbolId;
use std::collections::HashMap;

/// The mutable container types. Used to spot the ONE cross-unit arm this pass
/// leaves alone: a mutable in-place `del` (`%del-*-mut`), which keeps running
/// through the `del` wrapper's per-arm container compensation instead of
/// collapsing to the direct op. Every other store/remove op — including immutable
/// `del` and the fresh-result mutable funnels `%bytes-push`/`%pop-string` —
/// collapses on any container mutability. The exclusion is conservative, not
/// forced: the raw op self-reclaims (`raw-del` reads 0), so lifting it is a
/// separate, measurable step, gated on `del-wrapper`/`set-del-wrapper` staying at
/// 0 in `tests/elle/oracle.lisp`.
fn is_mutable_container(ty: TyId) -> bool {
    matches!(
        ty,
        TypeInterner::MUTABLE_STRUCT
            | TypeInterner::MUTABLE_ARRAY
            | TypeInterner::MUTABLE_STRING
            | TypeInterner::MUTABLE_BYTES
            | TypeInterner::MUTABLE_SET
    )
}

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

/// One arm of a wrapper summarized for the cross-unit registry: the container type
/// it selects on and the monomorphic op it routes to, named by `SymbolId` rather
/// than a `Binding`. A `Binding` is a per-arena index, meaningless in a later unit;
/// the op's name and the container `TyId` (a well-known `TypeInterner` constant) are
/// both stable, so the arm re-resolves against the consuming unit's own primitive
/// bindings. The operand map (`Arm::arg_src`) is not carried — it was already proven
/// to be the identity `0..arity` when the wrapper was collected, so the rewrite reuses
/// the call's args in order.
pub(crate) struct RegArm {
    pub(crate) ty: TyId,
    pub(crate) native_name: SymbolId,
    /// This arm does not monomorphize cross-unit — it is a mutable in-place `del`
    /// and stays on the wrapper's container compensation (see
    /// `is_mutable_container`). Decided at record time because an arm's `ty` is
    /// its fixed container type.
    skip: bool,
}

/// A wrapper summarized by name for cross-unit reuse (see `RegArm`).
pub(crate) struct RegWrapper {
    pub(crate) arity: usize,
    pub(crate) arms: Vec<RegArm>,
}

/// Per-instance persistent map of dispatch wrappers, keyed by wrapper NAME. Each
/// unit's `monomorphize_dispatch_wrappers` records its locally-defined wrappers
/// here (the stdlib's `push`/`put` land in it when `stdlib.lisp` compiles), and
/// every later unit consults it, so a user→stdlib wrapper call monomorphizes
/// exactly as an intra-unit one does — the F1b close, without a compensation gate.
///
/// This is compile-time-only state: the rewrite it drives leaves the direct op in
/// the HIR, so nothing here reaches the runtime. It rides on `CompileCtx` (the
/// per-instance compile context) precisely because it must outlive the single
/// compile that defined the wrapper — never on any VM/region structure.
#[derive(Default)]
pub struct DispatchWrapperRegistry {
    pub(crate) by_name: HashMap<SymbolId, RegWrapper>,
}

impl DispatchWrapperRegistry {
    /// Record a locally-collected wrapper under its name. First definition wins,
    /// so the stdlib's canonical wrapper is never clobbered by a later same-named
    /// user binding, and re-recording across compiles is a cheap no-op.
    fn record(
        &mut self,
        name: SymbolId,
        w: &Wrapper,
        arena: &BindingArena,
        symbol_names: &HashMap<u32, String>,
    ) {
        self.by_name.entry(name).or_insert_with(|| RegWrapper {
            arity: w.arity,
            arms: w
                .arms
                .iter()
                .map(|a| {
                    let native_name = arena.get(a.native).name;
                    let is_del = symbol_names
                        .get(&native_name.0)
                        .is_some_and(|n| n.starts_with("%del"));
                    RegArm {
                        ty: a.ty,
                        native_name,
                        skip: is_mutable_container(a.ty) && is_del,
                    }
                })
                .collect(),
        });
    }
    /// Snapshot this registry for the stdlib disk cache. SymbolIds are
    /// per-process; names travel instead, re-interned on load. `TyId` is a
    /// well-known `TypeInterner` constant (stable across processes).
    pub(crate) fn to_stored(&self, symbols: &crate::symbol::SymbolTable) -> StoredDispatchRegistry {
        StoredDispatchRegistry {
            by_name: self
                .by_name
                .iter()
                .map(|(name, rw)| {
                    (
                        symbols.name(*name).unwrap_or("").to_string(),
                        StoredRegWrapper {
                            arity: rw.arity,
                            arms: rw
                                .arms
                                .iter()
                                .map(|a| StoredRegArm {
                                    ty: a.ty.0,
                                    native_name: symbols
                                        .name(a.native_name)
                                        .unwrap_or("")
                                        .to_string(),
                                    skip: a.skip,
                                })
                                .collect(),
                        },
                    )
                })
                .collect(),
        }
    }
    /// Restore a registry snapshot into this one (used by the stdlib disk
    /// cache load path; re-interns names in the loading process's table).
    pub(crate) fn restore(
        &mut self,
        stored: StoredDispatchRegistry,
        symbols: &mut crate::symbol::SymbolTable,
    ) {
        self.by_name.clear();
        for (name, rw) in stored.by_name {
            self.by_name.insert(
                symbols.intern(&name),
                RegWrapper {
                    arity: rw.arity,
                    arms: rw
                        .arms
                        .into_iter()
                        .map(|a| RegArm {
                            ty: TyId(a.ty),
                            native_name: symbols.intern(&a.native_name),
                            skip: a.skip,
                        })
                        .collect(),
                },
            );
        }
    }
}

/// Rewrite every container-dispatch wrapper call whose container argument's type is a
/// statically-proven concrete container to a direct call to the selected arm's
/// monomorphic op. Runs after the inference fixpoint (so `hir_types` is populated) and
/// before the intrinsic operand proofs (so the rewritten op is contract-checked).
///
/// Two wrapper sources are consulted. A wrapper DEFINED in this unit is matched by
/// its `Binding` (the intra-unit path — the stdlib's own `map`→`push` calls, or a
/// user's local wrapper). A wrapper defined in an EARLIER unit — the stdlib's
/// `push`/`put` seen from user code — is matched through `registry`, keyed by name
/// and gated on the callee being `is_primitive` (a `bind_primitives` stdlib-export
/// reference; a user redefinition shadows it with a non-primitive binding and is
/// left alone). Locally-collected wrappers are recorded into `registry` here, so
/// each unit both populates it (for later units) and consumes it.
pub(super) fn monomorphize_dispatch_wrappers(
    hir: &mut Hir,
    hir_types: &HashMap<HirId, TyId>,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
    typeof_aliases: &HashMap<Binding, Binding>,
    registry: &mut DispatchWrapperRegistry,
) {
    let mut wrappers: HashMap<Binding, Wrapper> = HashMap::new();
    collect_wrappers(hir, arena, symbol_names, typeof_aliases, &mut wrappers);
    // Persist this unit's wrappers by name so later units can reach them (the
    // stdlib's push/put populate the instance registry on the `<stdlib>` compile).
    for (b, w) in &wrappers {
        registry.record(arena.get(*b).name, w, arena, symbol_names);
    }
    if wrappers.is_empty() && registry.by_name.is_empty() {
        return;
    }
    // A name→binding map of THIS unit's primitives, so a cross-unit arm's op (stored
    // by name) resolves to this arena's binding for it. `bind_primitives` binds each
    // primitive/stdlib-export once, so first-wins is exact.
    let mut prim_by_name: HashMap<SymbolId, Binding> = HashMap::new();
    for i in 0..arena.len() as u32 {
        let b = Binding(i);
        let bi = arena.get(b);
        if bi.is_primitive {
            prim_by_name.entry(bi.name).or_insert(b);
        }
    }
    rewrite(hir, hir_types, &wrappers, registry, &prim_by_name, arena);
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
    // Functionalize lists the rest binding in BOTH `params` and `rest_param`. The
    // logical fixed-param list — what an arm operand maps onto by position — excludes
    // it, and the `(first rest)` value operand (`put`'s 3-arg case) maps to the index
    // right AFTER the fixed params. Strip the rest binding so `fixed.len()` IS that
    // index; leaving it in overcounts by one and disqualifies every `& rest` wrapper.
    let fixed: Vec<Binding> = params
        .iter()
        .copied()
        .filter(|p| Some(*p) != rest_param)
        .collect();
    let param0 = *fixed.first()?;
    let rest_first = rest_param.and_then(|rp| find_rest_first_local(body, rp, arena, symbol_names));
    let arms = find_arms(
        body,
        param0,
        &fixed,
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
fn rewrite(
    hir: &mut Hir,
    hir_types: &HashMap<HirId, TyId>,
    wrappers: &HashMap<Binding, Wrapper>,
    registry: &DispatchWrapperRegistry,
    prim_by_name: &HashMap<SymbolId, Binding>,
    arena: &BindingArena,
) {
    hir.for_each_child_mut(|c| rewrite(c, hir_types, wrappers, registry, prim_by_name, arena));

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
    // Only a call whose args map 1:1 onto the arms' operands (no splices).
    if args.iter().any(|a| a.spliced) {
        return;
    }
    let Some(container) = args.first() else {
        return;
    };
    let Some(&cty) = hir_types.get(&arg_type_id(&container.expr)) else {
        return;
    };

    // Select the monomorphic op (a `Binding` in THIS arena) for this container type,
    // from the intra-unit wrapper (matched by binding) or the cross-unit registry
    // (matched by name, gated on the callee being a `is_primitive` stdlib-export
    // reference so a user redefinition is never mis-rewritten). No arm for `cty` —
    // dynamic or a non-container type — leaves the call intact.
    let native = if let Some(w) = wrappers.get(&wrapper_b) {
        if args.len() != w.arity {
            return;
        }
        match w.arms.iter().find(|a| a.ty == cty) {
            Some(arm) => arm.native,
            None => return,
        }
    } else if arena.get(wrapper_b).is_primitive {
        // Cross-unit: collapse the wrapper to the proven arm's op, EXCEPT a mutable
        // in-place `del` (`arm.skip`) — it stays on the wrapper's container
        // compensation (see `is_mutable_container`).
        let Some(rw) = registry.by_name.get(&arena.get(wrapper_b).name) else {
            return;
        };
        if args.len() != rw.arity {
            return;
        }
        let Some(arm) = rw.arms.iter().find(|a| a.ty == cty) else {
            return;
        };
        if arm.skip {
            return;
        }
        match prim_by_name.get(&arm.native_name).copied() {
            Some(b) => b,
            None => return,
        }
    } else {
        return;
    };

    // Build the monomorphic call: the arm's op over this call's args, in order. Fresh
    // nodes (fresh HirIds) so the later region walk's per-id side tables do not collide
    // with the replaced wrapper call's id. The arg nodes are reused as-is, keeping their
    // inferred types for the operand-proof check that follows.
    let is_tail = *is_tail;
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

#[cfg(test)]
mod tests {
    use crate::hir::arena::BindingArena;
    use crate::hir::expr::{Hir, HirKind};
    use std::collections::HashMap;

    /// Collect the name of every call callee (through the ANF/`Var` wrappers) in
    /// the tree, so a test can assert which ops a source form lowered to.
    fn callee_names(
        h: &Hir,
        arena: &BindingArena,
        names: &HashMap<u32, String>,
        out: &mut Vec<String>,
    ) {
        if let HirKind::Call { func, .. } = &h.kind {
            if let Some(b) = super::unwrap_callee_binding(func) {
                if let Some(n) = names.get(&arena.get(b).name.0) {
                    out.push(n.clone());
                }
            }
        }
        h.for_each_child(|c| callee_names(c, arena, names, out));
    }

    /// Cross-unit dispatch-wrapper monomorphization: a user call to the stdlib
    /// `put` wrapper on a statically-proven `:struct` must collapse to the direct
    /// `%put-struct` op — even though `put`'s definition lives in the stdlib
    /// compile unit, not the caller's. This is the F1b close: no surviving wrapper
    /// means no stranded owned-param container reference (the immutable residual),
    /// with no compensation gate. Fails before the cross-unit wrapper registry
    /// lands (the call stays a `put` wrapper call, leaking 1 region/op —
    /// `oracle.lisp` `native-tail-put-struct`).
    #[test]
    fn cross_unit_put_on_proven_struct_monomorphizes() {
        let mut rt = crate::runtime::Runtime::new(); // stdlib loaded
        let (_vm, symbols, cctx) = rt.parts();
        let (hir, arena, names) =
            crate::pipeline::compile_file_to_fhir("(put {:a 1} :b 2)", symbols, cctx, "<test>")
                .expect("compile");
        let mut callees = Vec::new();
        callee_names(&hir, &arena, &names, &mut callees);
        assert!(
            callees.iter().any(|n| n == "%put-struct"),
            "a `put` on a proven :struct must collapse to %put-struct; callees were {:?}",
            callees,
        );
        assert!(
            !callees.iter().any(|n| n == "put"),
            "the polymorphic `put` wrapper call must be gone after monomorphization; \
             callees were {:?}",
            callees,
        );
    }

    /// The store family beyond `put`: `push`/`add` on a proven immutable container
    /// collapse cross-unit to their monomorphic op the same way, through the same
    /// registry with no per-op change. Guards that the mechanism is generic over the
    /// store wrappers, not special-cased to `put`.
    #[test]
    fn cross_unit_push_add_on_proven_immutable_monomorphize() {
        let cases = [
            ("(push [1 2] 3)", "%push-array", "push"),
            ("(add (set 1 2) 3)", "%add-set", "add"),
            ("(push \"ab\" \"c\")", "%string-push", "push"),
        ];
        for (src, want_op, wrapper) in cases {
            let mut rt = crate::runtime::Runtime::new();
            let (_vm, symbols, cctx) = rt.parts();
            let (hir, arena, names) =
                crate::pipeline::compile_file_to_fhir(src, symbols, cctx, "<test>")
                    .expect("compile");
            let mut callees = Vec::new();
            callee_names(&hir, &arena, &names, &mut callees);
            assert!(
                callees.iter().any(|n| n == want_op),
                "{src} must collapse to {want_op}; callees were {callees:?}",
            );
            assert!(
                !callees.iter().any(|n| n == wrapper),
                "the `{wrapper}` wrapper call must be gone in {src}; callees were {callees:?}",
            );
        }
    }
}

/// Serializable snapshot of [`DispatchWrapperRegistry`] for the stdlib disk
/// cache. Names (not per-process `SymbolId`s) travel; re-interned on load.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct StoredDispatchRegistry {
    pub(crate) by_name: Vec<(String, StoredRegWrapper)>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredRegWrapper {
    pub(crate) arity: usize,
    pub(crate) arms: Vec<StoredRegArm>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredRegArm {
    pub(crate) ty: u32,
    pub(crate) native_name: String,
    pub(crate) skip: bool,
}
