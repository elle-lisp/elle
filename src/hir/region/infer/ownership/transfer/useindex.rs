//! Per-unit facts the producer/consumer gates read: every `Var` occurrence
//! classified by use form, each call site's innermost enclosing Lambda, and
//! each binding's inits — built in one structural walk over the HIR.

use super::*;

/// How a `Var(b)` occurrence is consumed — the use-form classification the
/// producer/consumer gates read. Everything the gates cannot prove harmless is
/// `Other` (a bare read: an alias, a store operand, a return, an intrinsic
/// argument — each a route a reference could escape the cut's reclamation
/// horizon through).
#[derive(Clone, Copy, PartialEq, Eq)]
pub(super) enum UseForm {
    /// The callee position of a call: `(f …)`.
    Callee(HirId),
    /// Argument 0 of a `fiber/new` call (the body closure).
    FiberNewArg0(HirId),
    /// Argument 0 of a `fiber/resume` call (the fiber being driven).
    ResumeArg0(HirId),
    /// An argument of a native whose declared `RegionEffect` is `Immediate` —
    /// the result is an immediate and no argument is stored or aliased, so the
    /// read is harmless (`fiber/status`, `fiber/set-fuel`).
    ImmediateArg,
    /// Any other occurrence.
    Other,
}

/// Per-unit facts the gates read repeatedly: every `Var` occurrence classified
/// by use form, each call site's innermost enclosing Lambda, and each
/// binding's inits. Child nodes are stored as `*const Hir` — the established
/// idiom for keeping HIR references across a `for_each_child` walk (the tree
/// outlives the analysis; see `RegionInference::binding_lambda`).
pub(super) struct UseIndex {
    /// Binding → its classified occurrences, in structural walk order.
    pub uses: FxHashMap<Binding, Vec<UseForm>>,
    /// Binding → EVERY init expression a Let/Letrec/Define/Destructure gives
    /// it, in walk order. A binding is a producer / fiber-body candidate only
    /// when it has exactly one init and that init is a Lambda — a sibling
    /// re-def or a destructure target has no stable binding→lambda identity.
    pub inits: FxHashMap<Binding, Vec<*const Hir>>,
    /// Call-node → innermost enclosing Lambda HirId (`None` = the top level).
    pub enclosing: FxHashMap<HirId, Option<HirId>>,
    /// Call-node → the bindings bound (directly, or through the ANF producer
    /// wrapper) to that call's value. A call with no entry was consumed at a
    /// non-binding position: a statement discard (safe), or a tail (a
    /// region-level return seed the consumer gate refuses independently).
    pub bound_to: FxHashMap<HirId, Vec<Binding>>,
}

impl UseIndex {
    pub(super) fn build(hir: &Hir, arena: &BindingArena, cc: &CallClassification) -> Self {
        let mut ix = UseIndex {
            uses: FxHashMap::default(),
            inits: FxHashMap::default(),
            enclosing: FxHashMap::default(),
            bound_to: FxHashMap::default(),
        };
        walk(hir, arena, cc, None, &mut ix);
        return ix;

        fn classify_arg(
            call: &Hir,
            func: &Hir,
            index: usize,
            arena: &BindingArena,
            cc: &CallClassification,
        ) -> UseForm {
            let sym = callee_symbol(func, arena);
            if index == 0 {
                if sym.is_some() && sym == cc.fiber_new {
                    return UseForm::FiberNewArg0(call.id);
                }
                if sym.is_some() && sym == cc.fiber_resume {
                    return UseForm::ResumeArg0(call.id);
                }
            }
            if callee_effect(func, arena, cc)
                == Some(crate::primitives::def::RegionEffect::Immediate)
            {
                return UseForm::ImmediateArg;
            }
            UseForm::Other
        }

        /// Record a binding's init: the init list (the binding→lambda
        /// identity) and, when the init's value is a call, the call→binding
        /// edge the consumer gate reads (`bound_to`).
        fn record_binding(ix: &mut UseIndex, b: Binding, init: &Hir) {
            ix.inits.entry(b).or_default().push(init as *const Hir);
            let tail = anf_tail(unwrap_cell(init));
            if matches!(tail.kind, HirKind::Call { .. }) {
                ix.bound_to.entry(tail.id).or_default().push(b);
            }
        }

        /// Walk an argument expression and return the binding its value
        /// position consumes: a bare `Var`, or the tail `Var` of an ANF
        /// producer wrapper (whose inits and non-tail statements are walked
        /// normally — only the tail read itself is classified by the caller
        /// instead of recorded as `Other`). `None` for any other tail, which
        /// is walked in full.
        fn arg_value_binding(
            h: &Hir,
            arena: &BindingArena,
            cc: &CallClassification,
            lambda: Option<HirId>,
            ix: &mut UseIndex,
        ) -> Option<Binding> {
            match &unwrap_cell(h).kind {
                HirKind::Var(b) => Some(*b),
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (b, init) in bindings {
                        record_binding(ix, *b, init);
                        walk(init, arena, cc, lambda, ix);
                    }
                    arg_value_binding(body, arena, cc, lambda, ix)
                }
                HirKind::Begin(es) => {
                    let (last, rest) = es.split_last()?;
                    for e in rest {
                        walk(e, arena, cc, lambda, ix);
                    }
                    arg_value_binding(last, arena, cc, lambda, ix)
                }
                _ => {
                    walk(h, arena, cc, lambda, ix);
                    None
                }
            }
        }

        fn walk(
            h: &Hir,
            arena: &BindingArena,
            cc: &CallClassification,
            lambda: Option<HirId>,
            ix: &mut UseIndex,
        ) {
            match &h.kind {
                HirKind::Var(b) => {
                    // A bare Var reached structurally (not intercepted by the
                    // Call arm below) is an unclassified read.
                    ix.uses.entry(*b).or_default().push(UseForm::Other);
                }
                HirKind::Call { func, args, .. } => {
                    ix.enclosing.insert(h.id, lambda);
                    match &unwrap_cell(func).kind {
                        HirKind::Var(b) => {
                            ix.uses.entry(*b).or_default().push(UseForm::Callee(h.id))
                        }
                        _ => walk(func, arena, cc, lambda, ix),
                    }
                    for (i, arg) in args.iter().enumerate() {
                        // The binding the arg position consumes, descending the
                        // ANF producer wrapper (whose inits/effects are walked
                        // normally); a non-binding tail is just walked.
                        if let Some(a) = arg_value_binding(&arg.expr, arena, cc, lambda, ix) {
                            let form = classify_arg(h, func, i, arena, cc);
                            ix.uses.entry(a).or_default().push(form);
                        }
                    }
                }
                HirKind::Lambda { body, .. } => {
                    // A capture is not a Var occurrence; only the body's reads
                    // count (captured bindings are gated separately through
                    // the structural capture sets).
                    walk(body, arena, cc, Some(h.id), ix);
                }
                HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
                    for (b, init) in bindings {
                        record_binding(ix, *b, init);
                        walk(init, arena, cc, lambda, ix);
                    }
                    // The ANF producer wrapper's self-tail read
                    // (`(let [t e] t)`) is pure value flow — the position the
                    // LET occupies is what consumes the value (an arg position
                    // classifies it through `arg_value_binding`; a statement
                    // discards it; a returned value is a region-level seed).
                    // Recording it as a read would mark every ANF-named value
                    // `Other` and refuse everything.
                    if let (1, HirKind::Var(v)) = (bindings.len(), &body.kind) {
                        if *v == bindings[0].0 {
                            return;
                        }
                    }
                    walk(body, arena, cc, lambda, ix);
                }
                HirKind::Define { binding, value } => {
                    record_binding(ix, *binding, value);
                    walk(value, arena, cc, lambda, ix);
                }
                HirKind::Destructure { pattern, value, .. } => {
                    // A destructure target's value identity is not a lambda
                    // init — record the (non-lambda) source as a poisoning
                    // init for each bound name.
                    for b in pattern.bindings().bindings {
                        ix.inits
                            .entry(b)
                            .or_default()
                            .push(value.as_ref() as *const Hir);
                    }
                    walk(value, arena, cc, lambda, ix);
                }
                _ => {
                    h.for_each_child(|c| walk(c, arena, cc, lambda, ix));
                }
            }
        }
    }

    /// The binding's sole Lambda init, when it is immutable, never mutated,
    /// and has exactly one recorded init that is a Lambda — the stable
    /// binding→lambda identity every gate below leans on.
    ///
    /// SAFETY of the deref: the pointers index nodes of the HIR tree, which
    /// outlives this analysis (both live for the `analyze_regions_with` call).
    pub(super) fn sole_lambda_init(&self, b: Binding, arena: &BindingArena) -> Option<&Hir> {
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return None;
        }
        match self.inits.get(&b).map(|v| v.as_slice()) {
            Some([l]) => {
                let l = unwrap_cell(unsafe { &**l });
                matches!(l.kind, HirKind::Lambda { .. }).then_some(l)
            }
            _ => None,
        }
    }
}
