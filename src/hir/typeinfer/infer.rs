use super::*;
use crate::hir::pattern::{HirPattern, PatternLiteral};

/// Collect which bindings are lambda definitions and what their params are.
pub(super) fn collect_lambda_info(
    hir: &Hir,
    _arena: &BindingArena,
    lambda_params: &mut HashMap<Binding, Vec<Binding>>,
) {
    match &hir.kind {
        HirKind::Letrec { bindings, body } | HirKind::Let { bindings, body } => {
            for (binding, value) in bindings {
                if let HirKind::Lambda { params, .. } = &value.kind {
                    lambda_params.insert(*binding, params.clone());
                }
                collect_lambda_info(value, _arena, lambda_params);
            }
            collect_lambda_info(body, _arena, lambda_params);
        }
        _ => {
            hir.for_each_child(|child| collect_lambda_info(child, _arena, lambda_params));
        }
    }
}

/// Forward type inference pass. Returns true if any types changed.
#[allow(clippy::too_many_arguments)]
pub(super) fn infer_types(
    hir: &Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    binding_types: &mut HashMap<Binding, TyId>,
    hir_types: &mut HashMap<HirId, TyId>,
    lambda_params: &HashMap<Binding, Vec<Binding>>,
    lambda_body_type: &mut HashMap<Binding, TyId>,
    symbol_names: &HashMap<u32, String>,
    binding_min_length: &mut HashMap<Binding, usize>,
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
    );
    let old = hir_types.insert(hir.id, ty);
    old != Some(ty)
}

/// Infer the type of a single HIR node.
#[allow(clippy::too_many_arguments)]
pub(super) fn infer_node(
    hir: &Hir,
    interner: &TypeInterner,
    arena: &BindingArena,
    binding_types: &mut HashMap<Binding, TyId>,
    hir_types: &mut HashMap<HirId, TyId>,
    lambda_params: &HashMap<Binding, Vec<Binding>>,
    lambda_body_type: &mut HashMap<Binding, TyId>,
    symbol_names: &HashMap<u32, String>,
    binding_min_length: &mut HashMap<Binding, usize>,
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
                let ty = recurse!(init);
                hir_types.insert(init.id, ty);
                // For lambda bindings, track their body's return type
                if let HirKind::Lambda { body: lam_body, .. } = &init.kind {
                    let body_ty = hir_types
                        .get(&lam_body.id)
                        .copied()
                        .unwrap_or(TypeInterner::TOP);
                    let old = lambda_body_type
                        .get(binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    let joined = interner.join(old, body_ty);
                    lambda_body_type.insert(*binding, joined);
                } else {
                    let old = binding_types
                        .get(binding)
                        .copied()
                        .unwrap_or(TypeInterner::BOTTOM);
                    let joined = interner.join(old, ty);
                    binding_types.insert(*binding, joined);
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
        HirKind::Lambda { body, .. } => {
            let body_ty = recurse!(body);
            hir_types.insert(body.id, body_ty);
            // We return Top for the lambda value itself — it's a closure
            TypeInterner::TOP
        }

        // If — join branches, with type guard narrowing
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            let _cond_ty = recurse!(cond);
            hir_types.insert(cond.id, _cond_ty);

            // Type guard narrowing: if cond is a type predicate call,
            // narrow the binding's type in the true branch
            let guard = extract_type_guard(cond, arena);
            let saved_types: Vec<(Binding, Option<TyId>)>;
            if let Some((binding, narrow_ty)) = guard {
                saved_types = vec![(binding, binding_types.get(&binding).copied())];
                let old = binding_types
                    .get(&binding)
                    .copied()
                    .unwrap_or(TypeInterner::TOP);
                let narrowed = interner.meet(old, narrow_ty);
                binding_types.insert(binding, narrowed);
            } else {
                saved_types = Vec::new();
            }

            let then_ty = recurse!(then_branch);
            hir_types.insert(then_branch.id, then_ty);

            // Restore type environment for else branch
            for (binding, saved) in &saved_types {
                match saved {
                    Some(ty) => {
                        binding_types.insert(*binding, *ty);
                    }
                    None => {
                        binding_types.remove(binding);
                    }
                }
            }

            let else_ty = recurse!(else_branch);
            hir_types.insert(else_branch.id, else_ty);

            interner.join(then_ty, else_ty)
        }

        // Match — the type-discriminating sibling of `If`. When the scrutinee is
        // `(type-of <var>)`, a keyword-literal arm (`:@array`/`:@struct`/…) proves
        // <var>'s concrete type+mutability inside that arm's body — the static
        // proof a monomorphic data op (`%push-array-mut`, …) needs to lower as a
        // silent opcode (pinned by the `match_typeof_narrows_*` tests). Children are
        // visited exactly as `for_each_child` (value, then each guard+body); the
        // narrowing is saved/restored per arm so it never leaks to a sibling — the
        // same discipline as the `If` then/else branches above. The Match node
        // itself stays `Top`: its value is an arm join no consumer relies on, and
        // keeping it `Top` leaves every other inference result unchanged.
        HirKind::Match { value, arms } => {
            let val_ty = recurse!(value);
            hir_types.insert(value.id, val_ty);

            let subject = typeof_subject_binding(value, arena, symbol_names);
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
                    .and_then(|b| pattern_type_keyword(pat).map(|ty| (b, ty)))
                    .map(|(b, narrow_ty)| {
                        let prev = binding_types.get(&b).copied();
                        binding_types.insert(b, narrow_ty);
                        (b, prev)
                    });
                let body_ty = recurse!(body);
                hir_types.insert(body.id, body_ty);
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
            TypeInterner::TOP
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
            let callee_binding = unwrap_callee_binding(func);
            if let Some(callee_binding) = callee_binding {
                if let Some(params) = lambda_params.get(&callee_binding) {
                    for (i, param) in params.iter().enumerate() {
                        if let Some(&arg_ty) = arg_types.get(i) {
                            // Don't forward Top — it poisons the parameter type
                            // and prevents convergence in recursive functions.
                            if arg_ty != TypeInterner::TOP {
                                let old = binding_types
                                    .get(param)
                                    .copied()
                                    .unwrap_or(TypeInterner::BOTTOM);
                                let joined = interner.join(old, arg_ty);
                                binding_types.insert(*param, joined);
                            }
                        }
                    }
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

        // Begin/Block — type is last expression
        HirKind::Begin(exprs) => {
            let mut ty = TypeInterner::NIL;
            for expr in exprs {
                ty = recurse!(expr);
                hir_types.insert(expr.id, ty);
            }
            ty
        }
        HirKind::Block { body, .. } => {
            let mut ty = TypeInterner::NIL;
            for expr in body {
                ty = recurse!(expr);
                hir_types.insert(expr.id, ty);
            }
            ty
        }

        // And/Or — conservative: Top
        HirKind::And(_) | HirKind::Or(_) => {
            hir.for_each_child(|child| {
                let ty = recurse!(child);
                hir_types.insert(child.id, ty);
            });
            TypeInterner::TOP
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

        // Assign/Define — update binding type
        HirKind::Assign { target, value }
        | HirKind::Define {
            binding: target,
            value,
        } => {
            let ty = recurse!(value);
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

/// The binding a `(type-of <var>)` match scrutinee discriminates, for match-arm
/// narrowing. Recognizes both the `type-of`/`type` callable and the `%type-of`
/// intrinsic; the subject must be a `Var` or `DerefCell { Var }` (a letrec/cell
/// read), mirroring `extract_type_guard`'s subject extraction.
///
/// ANF names the scrutinee, so `(match (type-of c) …)` reaches here as
/// `(match (let [t (type-of c)] t) …)` — the `value` is a `Let` whose body reads
/// the named call result, not the call itself. `unwrap_anf_let` follows that
/// binding back to the underlying expression before matching `type-of`.
pub(super) fn typeof_subject_binding(
    value: &Hir,
    arena: &BindingArena,
    symbol_names: &HashMap<u32, String>,
) -> Option<Binding> {
    let inner = unwrap_anf_let(value);
    let subject = match &inner.kind {
        HirKind::Intrinsic {
            op: IntrinsicOp::TypeOf,
            args,
        } if args.len() == 1 => &args[0],
        HirKind::Call { func, args, .. } if args.len() == 1 => {
            let callee = unwrap_callee_binding(func)?;
            let name = symbol_names.get(&arena.get(callee).name.0)?;
            if name != "type-of" && name != "type" {
                return None;
            }
            &args[0].expr
        }
        _ => return None,
    };
    var_of(subject)
}

/// A `Var(b)` or `DerefCell { Var(b) }` reference's binding, else `None`.
fn var_of(h: &Hir) -> Option<Binding> {
    match &h.kind {
        HirKind::Var(b) => Some(*b),
        HirKind::DerefCell { cell } => match &cell.kind {
            HirKind::Var(b) => Some(*b),
            _ => None,
        },
        _ => None,
    }
}

/// Follow an ANF-introduced `(let [t EXPR] t)` wrapper to `EXPR`. ANF names a
/// non-trivial scrutinee, binding it once and returning the bound var; this
/// unwraps that single-use naming (iteratively, in case of nesting) so the
/// underlying `type-of` call is visible. A `Let` whose body is not one of its
/// own bindings is returned as-is.
fn unwrap_anf_let(h: &Hir) -> &Hir {
    let mut cur = h;
    while let HirKind::Let { bindings, body } = &cur.kind {
        let Some(b) = var_of(body) else { break };
        let Some((_, init)) = bindings.iter().find(|(bb, _)| *bb == b) else {
            break;
        };
        cur = init;
    }
    cur
}

/// The container family a monomorphic data op trusts its first argument to be,
/// as the pair `(family-name, accepted TyIds)`. `None` for ops with no such
/// obligation — arithmetic, predicates, and the *polymorphic* `%put`/`%array-push`,
/// which dispatch on the runtime value rather than a static proof.
///
/// Both mutabilities are accepted: this slice's gate is *legality* (the container
/// type is statically known, so the silent opcode is type-sound), not the tighter
/// mutability proof that the region-precision slice (the `funnel_store_edges`
/// work) will demand of the `-mut` variants. Pinning only the family keeps the
/// landed `push_array_*`/`put_*` return-type tests — which deliberately feed an
/// opposite-mutability literal to attribute the result type to the op — green.
fn monomorphic_container_family(op: IntrinsicOp) -> Option<(&'static str, [TyId; 2])> {
    match op {
        IntrinsicOp::PushArray
        | IntrinsicOp::PushArrayMut
        | IntrinsicOp::PutArray
        | IntrinsicOp::PutArrayMut => {
            Some(("array", [TypeInterner::ARRAY, TypeInterner::MUTABLE_ARRAY]))
        }
        IntrinsicOp::PutStruct | IntrinsicOp::PutStructMut => Some((
            "struct",
            [TypeInterner::STRUCT, TypeInterner::MUTABLE_STRUCT],
        )),
        _ => None,
    }
}

/// The monomorphization proof obligation: in
/// silent (unchecked-intrinsics) context a monomorphic container op lowers to a
/// raw opcode with **no runtime type guard**, so its container argument must be
/// statically proven a container of the op's family. Walk the tree and reject any
/// site whose first argument's inferred type (`hir_types`, which after match-arm
/// narrowing records the *narrowed* container type per occurrence) is not in that
/// family — `Top` (an unproven binding) and any non-container type alike.
///
/// Runs only on the silent path: `infer_and_rewrite` early-returns under
/// `--checked-intrinsics`, where the op instead routes through its type-checking
/// `NativeFn` and the runtime guard catches a mismatch. Pinned by
/// `silent_unproven_monomorphic_op_is_compile_error` (the unproven binding) and
/// `proven_monomorphic_op_compiles_under_match_narrowing` (the discharged case).
pub(super) fn check_monomorphic_proof_obligations(
    hir: &Hir,
    hir_types: &HashMap<HirId, TyId>,
) -> Result<(), String> {
    if let HirKind::Intrinsic { op, args } = &hir.kind {
        if let Some((family, accepted)) = monomorphic_container_family(*op) {
            let arg_ty = args
                .first()
                .and_then(|a| hir_types.get(&a.id).copied())
                .unwrap_or(TypeInterner::TOP);
            if !accepted.contains(&arg_ty) {
                return Err(format!(
                    "{}: {}: container argument is not a statically-proven {} — a \
                     monomorphic container op requires its container proven in silent \
                     (unchecked-intrinsics) context",
                    hir.span,
                    op.name(),
                    family,
                ));
            }
        }
    }
    let mut result = Ok(());
    hir.for_each_child(|child| {
        if result.is_ok() {
            result = check_monomorphic_proof_obligations(child, hir_types);
        }
    });
    result
}

/// The concrete container type a keyword-literal arm pattern proves for a
/// `(type-of x)` scrutinee — array/struct/string/bytes/set in both mutabilities.
fn pattern_type_keyword(pat: &HirPattern) -> Option<TyId> {
    let HirPattern::Literal(PatternLiteral::Keyword(s)) = pat else {
        return None;
    };
    // The reader stores keyword patterns without the leading `:`; tolerate both.
    let s = s.strip_prefix(':').unwrap_or(s);
    match s {
        "@array" => Some(TypeInterner::MUTABLE_ARRAY),
        "@struct" => Some(TypeInterner::MUTABLE_STRUCT),
        "@string" => Some(TypeInterner::MUTABLE_STRING),
        "@bytes" => Some(TypeInterner::MUTABLE_BYTES),
        "array" => Some(TypeInterner::ARRAY),
        "struct" => Some(TypeInterner::STRUCT),
        "string" => Some(TypeInterner::STRING),
        "bytes" => Some(TypeInterner::BYTES),
        "@set" => Some(TypeInterner::MUTABLE_SET),
        "set" => Some(TypeInterner::SET),
        _ => None,
    }
}
