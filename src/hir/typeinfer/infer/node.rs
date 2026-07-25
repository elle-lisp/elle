use super::super::*;
use super::*;

/// Forward type inference pass. Returns true if any types changed.
#[allow(clippy::too_many_arguments)]
pub(crate) fn infer_types(
    hir: &Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    binding_types: &mut HashMap<Binding, TyId>,
    hir_types: &mut HashMap<HirId, TyId>,
    lambda_params: &HashMap<Binding, Vec<Binding>>,
    lambda_body_type: &mut HashMap<Binding, TyId>,
    symbol_names: &HashMap<u32, String>,
    binding_min_length: &mut HashMap<Binding, usize>,
    value_used: &std::collections::HashSet<Binding>,
    typeof_aliases: &HashMap<Binding, Binding>,
    param_joins: &mut HashMap<Binding, TyId>,
) -> bool {
    let ty = infer_node(
        hir,
        interner,
        arena,
        binding_types,
        hir_types,
        lambda_params,
        lambda_body_type,
        symbol_names,
        binding_min_length,
        &mut Vec::new(),
        value_used,
        typeof_aliases,
        param_joins,
    );
    let old = hir_types.insert(hir.id, ty);
    old != Some(ty)
}

/// Infer the type of a single HIR node.
#[allow(clippy::too_many_arguments)]
pub(crate) fn infer_node(
    hir: &Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    binding_types: &mut HashMap<Binding, TyId>,
    hir_types: &mut HashMap<HirId, TyId>,
    lambda_params: &HashMap<Binding, Vec<Binding>>,
    lambda_body_type: &mut HashMap<Binding, TyId>,
    symbol_names: &HashMap<u32, String>,
    binding_min_length: &mut HashMap<Binding, usize>,
    // The letrec lambdas whose own bodies are currently being inferred. A
    // call to one of these from within its own body contributes BOTTOM to
    // the type flow (the recursive contribution to a return-type join is
    // exactly the base cases — Kleene iteration from below), which is what
    // keeps a self-recursive body from poisoning itself with Top before any
    // call site has forwarded its parameter types.
    selfrec: &mut Vec<Binding>,
    // Bindings with at least one value-position use (`collect_value_position_uses`):
    // their callers are not all visible, so call-site joins must not prove
    // their parameters.
    value_used: &std::collections::HashSet<Binding>,
    // Immutable let-bound aliases of `(type-of a)`, mapped to their subject `a`
    // (`collect_typeof_aliases`), so a match on such an alias narrows `a`.
    typeof_aliases: &HashMap<Binding, Binding>,
    // This pass's call-site contributions to parameter types. RECOMPUTED per
    // pass (the driver replaces each contributed parameter's binding type
    // wholesale at pass end): a join that only ever accumulates can never
    // come back down, so unknown-typed call sites would have to be skipped —
    // and a skipped unknown is exactly the unsound "typed callers alone prove
    // the parameter" hole. Here Top contributes honestly.
    param_joins: &mut HashMap<Binding, TyId>,
) -> TyId {
    macro_rules! recurse {
        ($e:expr) => {
            infer_node(
                $e,
                interner,
                arena,
                binding_types,
                hir_types,
                lambda_params,
                lambda_body_type,
                symbol_names,
                binding_min_length,
                selfrec,
                value_used,
                typeof_aliases,
                param_joins,
            )
        };
    }

    match &hir.kind {
        // Literals
        HirKind::Nil => TypeInterner::NIL,
        HirKind::Bool(_) => TypeInterner::BOOL,
        HirKind::Int(_) => TypeInterner::INT,
        HirKind::Float(_) => TypeInterner::FLOAT,
        HirKind::String(_) => TypeInterner::STRING,
        HirKind::Keyword(_) => TypeInterner::KEYWORD,
        HirKind::EmptyList => TypeInterner::EMPTY_LIST,

        // Variable reference
        HirKind::Var(binding) => binding_types
            .get(binding)
            .copied()
            .unwrap_or(TypeInterner::TOP),

        // Intrinsic operations — known return types
        HirKind::Intrinsic { op, args } => {
            for arg in args {
                let ty = recurse!(arg);
                hir_types.insert(arg.id, ty);
            }
            intrinsic_return_type(*op, args, interner, hir_types)
        }

        // Let/Letrec — seed binding types from init values
        HirKind::Let { bindings, body } | HirKind::Letrec { bindings, body } => {
            for (binding, init) in bindings {
                let is_lambda_init = matches!(&unwrap_make_cell(init).kind, HirKind::Lambda { .. });
                if is_lambda_init {
                    selfrec.push(*binding);
                }
                let ty = recurse!(init);
                if is_lambda_init {
                    selfrec.pop();
                }
                hir_types.insert(init.id, ty);
                // For lambda bindings (possibly cell-wrapped when
                // self-recursive/captured), track their body's return type.
                // REPLACE, don't join: on the first pass a self-recursive
                // body reads its own parameters before any call site has
                // forwarded them, so it computes Top — and a join can never
                // come back down from Top on the later passes that know
                // better. Each pass re-derives the body type from strictly
                // more information; the last pass's value is the answer.
                if let HirKind::Lambda { body: lam_body, .. } = &unwrap_make_cell(init).kind {
                    let body_ty = hir_types
                        .get(&lam_body.id)
                        .copied()
                        .unwrap_or(TypeInterner::TOP);
                    lambda_body_type.insert(*binding, body_ty);
                } else {
                    let old = binding_types
                        .get(binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    let joined = interner.join(old, ty);
                    // A `(numeric!)`-declared binding keeps its floor here: this is
                    // the form a kernel parameter takes once HOF fusion has spliced
                    // its body into a loop (`(let [x (get coll i)] BODY)`), where
                    // the init's own type carries no proof.
                    binding_types
                        .insert(*binding, declared_floor(*binding, joined, arena, interner));
                    // Track min_length for array constructor bindings
                    if ty == TypeInterner::MUTABLE_ARRAY || ty == TypeInterner::ARRAY {
                        if let Some(len) = unwrap_to_call(init) {
                            binding_min_length.insert(*binding, len);
                        }
                    }
                }
            }
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            body_ty
        }

        // Lambda — infer body type and track return type
        HirKind::Lambda { params, body, .. } => {
            // A `(numeric!)` declaration is the programmer's numeric contract for
            // the whole body (the GPU-eligibility gate holds the lowered code to
            // it), so it proves every parameter ⊑ Number for the operand contracts
            // — the declared analog of a call-site join. It is read from the
            // parameter bindings, the single place it is recorded. An undeclared
            // parameter is left ALONE — absence from the environment is meaningful
            // (the driver's Kleene start: an unproven parameter reads as Top, a
            // provable one is seeded at Bottom).
            for p in params.iter().filter(|p| arena.get(**p).declared_numeric) {
                let old = binding_types.get(p).copied().unwrap_or(TypeInterner::TOP);
                binding_types.insert(*p, declared_floor(*p, old, arena, interner));
            }
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            // We return Top for the lambda value itself — it's a closure
            TypeInterner::TOP
        }

        // If — join branches, with guard narrowing on both sides: the
        // then-branch gets the condition's truthy facts (`(%int? x)` refines
        // x to Int), the else-branch its falsy facts (`(%not (number? b))`
        // false means b IS a number). `meet` because a predicate refines a
        // union (`Number ∧ Int = Int`) rather than fully determining the type
        // (contrast the authoritative `match (type-of x)` override below).
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let _cond_ty = recurse!(cond);
            hir_types.insert(cond.id, _cond_ty);

            let facts = super::super::guard::cond_facts(cond, arena, symbol_names);

            let saved = apply_type_facts(&facts.when_true, binding_types, interner);
            let then_ty = recurse!(then_branch);
            hir_types.insert(then_branch.id, then_ty);
            restore_type_facts(saved, binding_types);

            let saved = apply_type_facts(&facts.when_false, binding_types, interner);
            let else_ty = recurse!(else_branch);
            hir_types.insert(else_branch.id, else_ty);
            restore_type_facts(saved, binding_types);

            interner.join(then_ty, else_ty)
        }

        // Match — the type-discriminating sibling of `If`. When the scrutinee is
        // `(type-of <var>)`, a keyword-literal arm (`:@array`/`:@struct`/…) proves
        // <var>'s concrete type+mutability inside that arm's body — the static
        // proof a monomorphic data op (`%push-array-mut`, …) needs to lower as a
        // silent opcode (pinned by the `match_typeof_narrows_*` tests). Children are
        // visited exactly as `for_each_child` (value, then each guard+body); the
        // narrowing is saved/restored per arm so it never leaks to a sibling — the
        // same discipline as the `If` then/else branches above. The Match node's
        // own type is the join of its arm bodies: a type-dispatch helper that
        // returns the same type from every arm (the stdlib `compare`'s `rank`,
        // all-Int) carries that type to its callers, which is what proves the
        // downstream intrinsic operands.
        HirKind::Match { value, arms } => {
            let val_ty = recurse!(value);
            hir_types.insert(value.id, val_ty);

            let subject = typeof_subject_binding(value, arena, symbol_names, typeof_aliases);
            let mut arm_join = TypeInterner::BOTTOM;
            for (pat, guard, body) in arms {
                if let Some(g) = guard {
                    let g_ty = recurse!(g);
                    hir_types.insert(g.id, g_ty);
                }
                // Narrow the scrutinee's binding to the arm's proven container
                // type for the duration of the body, then restore. The narrowing
                // **overrides** the binding's accumulated type rather than `meet`ing
                // with it: a `(type-of x)` arm is *authoritative* — the runtime
                // dispatch guarantees `x`'s concrete type whenever the arm runs, so
                // within the body `x` simply IS the arm's keyword type, regardless of
                // what the forward flow widened the binding to across all call sites.
                // `meet` would be wrong here: if the binding accumulated a *disjoint*
                // concrete type (a parameter called elsewhere with a different
                // container — exactly the stdlib `push`/`put` shape), `meet` collapses
                // to `Bottom`, leaving an immutable-arm container "unproven" and its
                // silent monomorphic op a spurious compile error on an arm that only
                // ever runs for that very type (pinned by
                // `match_typeof_arm_narrows_authoritatively_over_a_called_param`). This
                // differs from the `If` type-guard above, which `meet`s because a
                // predicate like `(%int? x)` *refines* a union (`Number ∧ Int = Int`)
                // rather than fully determining the type. Override is sound because
                // the container keyword types are flat — the keyword is the most
                // precise type, never a supertype of the accumulated one.
                let saved = subject
                    .zip(pattern_type_keyword(pat))
                    .map(|(b, narrow_ty)| {
                        let prev = binding_types.get(&b).copied();
                        binding_types.insert(b, narrow_ty);
                        (b, prev)
                    });
                let body_ty = recurse!(body);
                hir_types.insert(body.id, body_ty);
                arm_join = interner.join(arm_join, body_ty);
                if let Some((b, prev)) = saved {
                    match prev {
                        Some(t) => {
                            binding_types.insert(b, t);
                        }
                        None => {
                            binding_types.remove(&b);
                        }
                    }
                }
            }
            if arms.is_empty() {
                TypeInterner::TOP
            } else {
                arm_join
            }
        }

        // Call — forward arg types to callee params; result = callee return type
        HirKind::Call { func, args, .. } => {
            let _func_ty = recurse!(func);
            hir_types.insert(func.id, _func_ty);

            let arg_types: Vec<TyId> = args
                .iter()
                .map(|a| {
                    let ty = recurse!(&a.expr);
                    hir_types.insert(a.expr.id, ty);
                    ty
                })
                .collect();

            // Forward arg types to callee params.
            // Handle both Var(b) and DerefCell { Var(b) } (letrec recursive calls).
            // Forwarding is a complete proof only for a callee used EXCLUSIVELY
            // in call position — a single value-position use (stored, passed,
            // returned, exported) means invisible callers exist, so the joins
            // must not prove its parameters.
            let callee_binding = unwrap_callee_binding(func);
            if let Some(callee_binding) = callee_binding {
                if let Some(params) = lambda_params
                    .get(&callee_binding)
                    .filter(|_| !value_used.contains(&callee_binding))
                {
                    for (i, param) in params.iter().enumerate() {
                        if let Some(&arg_ty) = arg_types.get(i) {
                            // Top contributes honestly: an unknown-typed call
                            // site makes the parameter unprovable (the driver
                            // REPLACES the binding type from this map at pass
                            // end, so recursion converges from below instead
                            // of needing a Top skip).
                            let old = param_joins
                                .get(param)
                                .copied()
                                .unwrap_or(TypeInterner::BOTTOM);
                            param_joins.insert(*param, interner.join(old, arg_ty));
                        }
                    }
                }
                // A call to a lambda whose own body is being inferred (a
                // recursive call) contributes BOTTOM — the recursive
                // contribution to a return-type join is exactly the base
                // cases (Kleene iteration from below). It must NOT read the
                // running estimate: on the first pass that is Top (the body
                // was walked before any call site forwarded its parameters),
                // and Top can never come back down through a join. Argument
                // forwarding above still runs — a self-call is usually the
                // sole source of its own parameters' step types.
                if selfrec.contains(&callee_binding) {
                    return TypeInterner::BOTTOM;
                }
                // Return type = whatever the callee's body returns.
                // Only use BOTTOM for known lambdas (in lambda_params) where the
                // body type hasn't been computed yet. For unknown callees (primitives,
                // imports), return TOP to avoid unsound rewrites.
                if lambda_params.contains_key(&callee_binding) {
                    let ret_ty = lambda_body_type
                        .get(&callee_binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    return ret_ty;
                }

                // Primitive return type inference for unresolved callees
                let callee_sym = arena.get(callee_binding).name;
                if let Some(name) = symbol_names.get(&callee_sym.0) {
                    let prim_ty = primitive_return_type(name, &arg_types, interner);
                    if prim_ty != TypeInterner::TOP {
                        return prim_ty;
                    }
                }
            }

            TypeInterner::TOP
        }

        // Begin/Block — type is last expression. Flow-through guard
        // narrowing: a one-armed diverging guard statement —
        // `(when (%not (number? b)) (error …))` — proves its fall-through
        // facts for every statement after it (the stdlib wrapper shape: the
        // guard that raises the wrapper's :error is the same fact that
        // discharges the intrinsic's contract downstream). Facts are scoped
        // to the sequence and restored on exit.
        HirKind::Begin(exprs) => {
            let mut ty = TypeInterner::NIL;
            let mut saved = Vec::new();
            for expr in exprs {
                ty = recurse!(expr);
                hir_types.insert(expr.id, ty);
                let facts = super::super::guard::facts_after_statement(expr, arena, symbol_names);
                saved.extend(apply_type_facts(&facts, binding_types, interner));
            }
            restore_type_facts(saved, binding_types);
            ty
        }
        HirKind::Block { body, .. } => {
            let mut ty = TypeInterner::NIL;
            let mut saved = Vec::new();
            for expr in body {
                ty = recurse!(expr);
                hir_types.insert(expr.id, ty);
                let facts = super::super::guard::facts_after_statement(expr, arena, symbol_names);
                saved.extend(apply_type_facts(&facts, binding_types, interner));
            }
            restore_type_facts(saved, binding_types);
            ty
        }

        // Cond — sequential If chain: each clause body gets its test's truthy
        // facts; each later clause (and the else) additionally knows every
        // earlier test was falsy. Result is the join of all bodies.
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            let mut ty = TypeInterner::BOTTOM;
            let mut fallthrough_saved = Vec::new();
            for (test, body) in clauses {
                let test_ty = recurse!(test);
                hir_types.insert(test.id, test_ty);
                let facts = super::super::guard::cond_facts(test, arena, symbol_names);
                let saved = apply_type_facts(&facts.when_true, binding_types, interner);
                let body_ty = recurse!(body);
                hir_types.insert(body.id, body_ty);
                restore_type_facts(saved, binding_types);
                ty = interner.join(ty, body_ty);
                fallthrough_saved.extend(apply_type_facts(
                    &facts.when_false,
                    binding_types,
                    interner,
                ));
            }
            if let Some(els) = else_branch {
                let else_ty = recurse!(els);
                hir_types.insert(els.id, else_ty);
                ty = interner.join(ty, else_ty);
            }
            restore_type_facts(fallthrough_saved, binding_types);
            ty
        }

        // And/Or — conservative: Top
        // `and`/`or` evaluate to one of their operands — `and` the first falsy
        // (else the last), `or` the first truthy (else the last) — so the result
        // type is the JOIN of the operand types, exactly as `If` joins its
        // branches. Join is sound (a returned value is always ⊑ its operand's
        // type) and lets a homogeneous `(or a b)`/`(and a b)` of proven numbers
        // discharge a downstream `%`-intrinsic; a heterogeneous one widens and
        // correctly fails to prove. (Empty `and`/`or` never reach here — the
        // analyzer emits a Bool literal for them.)
        HirKind::And(exprs) | HirKind::Or(exprs) => {
            let mut join = TypeInterner::BOTTOM;
            for child in exprs {
                let ty = recurse!(child);
                hir_types.insert(child.id, ty);
                join = interner.join(join, ty);
            }
            join
        }

        // Loop — recurse into body
        HirKind::Loop { bindings, body } => {
            for (binding, init) in bindings {
                let ty = recurse!(init);
                hir_types.insert(init.id, ty);
                let old = binding_types
                    .get(binding)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
                binding_types.insert(*binding, interner.join(old, ty));
            }
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            body_ty
        }

        // Assign/Define — update binding type. A Define whose value is a
        // lambda (the in-function `defn` idiom — a letrec*-semantics local)
        // records its return type exactly like a Let/Letrec lambda binding,
        // with the same selfrec discipline; `collect_lambda_info` records its
        // params.
        HirKind::Assign { target, value }
        | HirKind::Define {
            binding: target,
            value,
        } => {
            let is_lambda_init = matches!(&unwrap_make_cell(value).kind, HirKind::Lambda { .. });
            if is_lambda_init {
                selfrec.push(*target);
            }
            let ty = recurse!(value);
            if is_lambda_init {
                selfrec.pop();
                if let HirKind::Lambda { body: lam_body, .. } = &unwrap_make_cell(value).kind {
                    let body_ty = hir_types
                        .get(&lam_body.id)
                        .copied()
                        .unwrap_or(TypeInterner::TOP);
                    lambda_body_type.insert(*target, body_ty);
                }
            }
            hir_types.insert(value.id, ty);
            let old = binding_types
                .get(target)
                .copied()
                .unwrap_or(TypeInterner::BOTTOM);
            binding_types.insert(*target, interner.join(old, ty));
            // Track min_length for array constructor bindings
            if ty == TypeInterner::MUTABLE_ARRAY || ty == TypeInterner::ARRAY {
                if let Some(call) = unwrap_to_call(value) {
                    binding_min_length.insert(*target, call);
                }
            }
            ty
        }

        // Quoted compound data has the type its template's outermost node
        // materializes to — a quoted proper list IS a pair chain, which is
        // what proves `(%first '(a b c))`.
        HirKind::QuoteConst(template) => {
            use crate::value::ConstTemplate;
            match template {
                ConstTemplate::Pair(_, _) => TypeInterner::PAIR,
                ConstTemplate::Array(_) => TypeInterner::ARRAY,
                ConstTemplate::ArrayMut(_) => TypeInterner::MUTABLE_ARRAY,
                ConstTemplate::String(_) => TypeInterner::STRING,
                ConstTemplate::StringMut(_) => TypeInterner::MUTABLE_STRING,
                ConstTemplate::EmptyList => TypeInterner::EMPTY_LIST,
                ConstTemplate::Int(_) => TypeInterner::INT,
                ConstTemplate::Float(_) => TypeInterner::FLOAT,
                ConstTemplate::Bool(_) => TypeInterner::BOOL,
                ConstTemplate::Keyword(_) => TypeInterner::KEYWORD,
                ConstTemplate::Symbol(_) => TypeInterner::SYMBOL,
                ConstTemplate::Nil => TypeInterner::NIL,
                _ => TypeInterner::TOP,
            }
        }

        // Return — the function-return ownership boundary is region-only and
        // type-transparent: the result is the same value. Without this arm a
        // lambda's body type (wrapped in Return by wrap_tail_returns) would
        // read Top and no callee return type would ever flow to callers.
        HirKind::Return { value } => {
            let ty = recurse!(value);
            hir_types.insert(value.id, ty);
            ty
        }

        // MakeCell — propagate inner value type
        HirKind::MakeCell { value } => {
            let ty = recurse!(value);
            hir_types.insert(value.id, ty);
            ty
        }

        // DerefCell — return binding type if cell is Var(b)
        HirKind::DerefCell { cell } => {
            let ty = recurse!(cell);
            hir_types.insert(cell.id, ty);
            if let HirKind::Var(b) = &cell.kind {
                binding_types.get(b).copied().unwrap_or(TypeInterner::TOP)
            } else {
                TypeInterner::TOP
            }
        }

        // SetCell — widen binding type
        HirKind::SetCell { cell, value } => {
            let cell_ty = recurse!(cell);
            hir_types.insert(cell.id, cell_ty);
            let val_ty = recurse!(value);
            hir_types.insert(value.id, val_ty);
            // Widen the binding's type with the new value
            if let HirKind::Var(b) = &cell.kind {
                let old = binding_types
                    .get(b)
                    .copied()
                    .unwrap_or(TypeInterner::BOTTOM);
                binding_types.insert(*b, interner.join(old, val_ty));
            }
            val_ty
        }

        // Everything else — recurse and return Top
        _ => {
            hir.for_each_child(|child| {
                let ty = recurse!(child);
                hir_types.insert(child.id, ty);
            });
            TypeInterner::TOP
        }
    }
}
