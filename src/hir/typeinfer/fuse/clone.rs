use super::*;

/// A same-unit function eligible for inlining into a fused HOF: its parameters
/// and body, held as an owned template. Because the definition persists (it stays
/// bound and may be used as a first-class value), its body cannot be moved out;
/// each call site clones this template with fresh bindings and HirIds (see
/// `clone_template`/`clone_fresh`).
pub(super) struct FnTemplate {
    pub(super) params: Vec<Binding>,
    pub(super) body: Hir,
}

/// Walk every `Let`/`Letrec`/`Define` binding (the same forms `prune::collect_inits`
/// visits) and record those bound to an inlineable lambda template. Mirrors the
/// singly-bound/immutable/unmutated discipline of the init-keyword proof.
pub(super) fn collect_inline_fns(
    hir: &Hir,
    arena: &BindingArena,
    out: &mut FxHashMap<Binding, FnTemplate>,
    seen: &mut FxHashSet<Binding>,
) {
    let mut record = |b: Binding, value: &Hir, out: &mut FxHashMap<Binding, FnTemplate>| {
        // A binding bound more than once has no single stable value — drop it.
        if !seen.insert(b) {
            out.remove(&b);
            return;
        }
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        if let Some(t) = fn_template(value, arena) {
            out.insert(b, t);
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
    hir.for_each_child(|c| collect_inline_fns(c, arena, out, seen));
}

/// The inlineable template of a lambda initializer, or `None`. A qualifying lambda
/// is non-capturing, has 1 or 2 fixed parameters (a `map`/`filter`/`count`/search element
/// or a `fold` accumulator+element — the use site checks the exact arity), no rest
/// parameter, unmutated parameters, and a `clone_fresh`-admissible body
/// (`is_inlineable_body` — the pure-expression forms plus `let`, so the clone
/// freshens the parameters and any `let`-bound bindings and nothing else). The body
/// is cloned into the template; each call site re-clones it with fresh bindings.
pub(super) fn fn_template(value: &Hir, arena: &BindingArena) -> Option<FnTemplate> {
    let HirKind::Lambda {
        params,
        rest_param,
        captures,
        body,
        assert_numeric,
        ..
    } = &value.kind
    else {
        return None;
    };
    if rest_param.is_some() || !captures.is_empty() || params.is_empty() || params.len() > 2 {
        return None;
    }
    if params.iter().any(|p| arena.get(*p).is_mutated) || !is_inlineable_body(body, *assert_numeric)
    {
        return None;
    }
    Some(FnTemplate {
        params: params.clone(),
        body: (**body).clone(),
    })
}

/// Is a body admissible for the alpha-renaming clone? The whitelist covers the
/// pure-expression forms plus `let`, and — under the function's own `(numeric!)`
/// declaration (`declared_numeric`) — a call-position `%`-intrinsic, whose
/// parameter proof the declaration carries onto the spliced binding
/// (docs/impl/dissolution.md § "Raw `%`-intrinsic bodies"). A pure-expression body
/// freshens the parameters, rewrites their references, and leaves every other
/// `Var` (a global) shared. A `let` additionally introduces bindings of its own —
/// those are freshened too (`clone_fresh`'s `Let` arm re-mints each `let`-bound
/// binding), so a `let` body clones without collision. `letrec` is **not** admitted
/// (its value may reference its own binding — a forward/self reference the
/// sequential rename cannot satisfy — and the recursive cell it builds is the shape
/// fusion avoids); a body with a `loop`/`match` binding or a nested lambda uses a
/// form not listed here and declines: the definition's own bindings are then never
/// duplicated (correct-by-construction). Kept in lockstep with `clone_fresh` — the
/// same variants, one returning `bool`, one rebuilding.
pub(super) fn is_inlineable_body(h: &Hir, num: bool) -> bool {
    // `num` is the function's `(numeric!)` declaration, threaded to every arm: it
    // gates the `Intrinsic` arm alone, and is inert everywhere else.
    match &h.kind {
        HirKind::Nil
        | HirKind::EmptyList
        | HirKind::Bool(_)
        | HirKind::Int(_)
        | HirKind::Float(_)
        | HirKind::String(_)
        | HirKind::Keyword(_)
        | HirKind::Var(_) => true,
        HirKind::Let { bindings, body } => {
            bindings.iter().all(|(_, v)| is_inlineable_body(v, num))
                && is_inlineable_body(body, num)
        }
        HirKind::Call { func, args, .. } => {
            is_inlineable_body(func, num) && args.iter().all(|a| is_inlineable_body(&a.expr, num))
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => {
            is_inlineable_body(cond, num)
                && is_inlineable_body(then_branch, num)
                && is_inlineable_body(else_branch, num)
        }
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            clauses
                .iter()
                .all(|(c, b)| is_inlineable_body(c, num) && is_inlineable_body(b, num))
                && else_branch
                    .as_ref()
                    .is_none_or(|e| is_inlineable_body(e, num))
        }
        HirKind::Begin(v) | HirKind::And(v) | HirKind::Or(v) => {
            v.iter().all(|c| is_inlineable_body(c, num))
        }
        HirKind::Intrinsic { args, .. } => num && args.iter().all(|a| is_inlineable_body(a, num)),
        _ => false,
    }
}

/// Deep-clone a whitelisted body with **fresh HirIds** (via `Hir::new` — a plain
/// `.clone()` would duplicate the global-counter ids and collide in the region
/// walk's per-id side tables) and **renamed bindings** (`renames`, old → fresh):
/// the parameters (seeded by `clone_template`) plus every `let`-bound binding the
/// body introduces (freshened in the `Let` arm as the clone descends). Every
/// non-renamed `Var` (a global) is left as-is. `renames` is threaded `&mut` so a
/// nested `let` can extend it, and `arena` `&mut` so a `let` binding can mint its
/// fresh id. Returns `None` on any form `is_inlineable_body` rejects — the two are
/// kept in lockstep, so a body that passed collection always clones.
pub(super) fn clone_fresh(
    h: &Hir,
    renames: &mut FxHashMap<Binding, Binding>,
    arena: &mut BindingArena,
) -> Option<Hir> {
    let kind = match &h.kind {
        HirKind::Nil => HirKind::Nil,
        HirKind::EmptyList => HirKind::EmptyList,
        HirKind::Bool(b) => HirKind::Bool(*b),
        HirKind::Int(n) => HirKind::Int(*n),
        HirKind::Float(f) => HirKind::Float(*f),
        HirKind::String(s) => HirKind::String(s.clone()),
        HirKind::Keyword(s) => HirKind::Keyword(s.clone()),
        HirKind::Var(b) => HirKind::Var(renames.get(b).copied().unwrap_or(*b)),
        // A `let` freshens its own bindings. Each value is cloned under the renames
        // established so far — before its binding is inserted — so a sequential
        // `let`'s later value sees the fresh id of an earlier binding, while a
        // binding's own value never renames to itself (that is `letrec`, excluded).
        // Each fresh binding is faithful to the source's mutability.
        HirKind::Let { bindings, body } => {
            let mut new_bindings = Vec::with_capacity(bindings.len());
            for (b, value) in bindings {
                let value = clone_fresh(value, renames, arena)?;
                let (is_immutable, is_mutated) = {
                    let bi = arena.get(*b);
                    (bi.is_immutable, bi.is_mutated)
                };
                let fresh = arena.gensym();
                let fi = arena.get_mut(fresh);
                fi.is_immutable = is_immutable;
                fi.is_mutated = is_mutated;
                renames.insert(*b, fresh);
                new_bindings.push((fresh, value));
            }
            let body = Box::new(clone_fresh(body, renames, arena)?);
            HirKind::Let {
                bindings: new_bindings,
                body,
            }
        }
        HirKind::Call {
            func,
            args,
            is_tail,
        } => {
            let func = Box::new(clone_fresh(func, renames, arena)?);
            let mut new_args = Vec::with_capacity(args.len());
            for a in args {
                new_args.push(CallArg {
                    expr: clone_fresh(&a.expr, renames, arena)?,
                    spliced: a.spliced,
                });
            }
            HirKind::Call {
                func,
                args: new_args,
                is_tail: *is_tail,
            }
        }
        HirKind::If {
            cond,
            then_branch,
            else_branch,
        } => HirKind::If {
            cond: Box::new(clone_fresh(cond, renames, arena)?),
            then_branch: Box::new(clone_fresh(then_branch, renames, arena)?),
            else_branch: Box::new(clone_fresh(else_branch, renames, arena)?),
        },
        HirKind::Cond {
            clauses,
            else_branch,
        } => {
            let mut cs = Vec::with_capacity(clauses.len());
            for (c, b) in clauses {
                cs.push((
                    clone_fresh(c, renames, arena)?,
                    clone_fresh(b, renames, arena)?,
                ));
            }
            let eb = match else_branch {
                Some(e) => Some(Box::new(clone_fresh(e, renames, arena)?)),
                None => None,
            };
            HirKind::Cond {
                clauses: cs,
                else_branch: eb,
            }
        }
        HirKind::Begin(v) => HirKind::Begin(clone_vec(v, renames, arena)?),
        HirKind::And(v) => HirKind::And(clone_vec(v, renames, arena)?),
        HirKind::Or(v) => HirKind::Or(clone_vec(v, renames, arena)?),
        // A raw `%`-intrinsic — admitted by `is_inlineable_body` only under the
        // function's `(numeric!)` declaration, which the fresh parameters carry.
        HirKind::Intrinsic { op, args } => HirKind::Intrinsic {
            op: *op,
            args: clone_vec(args, renames, arena)?,
        },
        _ => return None,
    };
    Some(Hir::new(kind, h.span.clone(), h.signal))
}

/// Clone a slice of whitelisted bodies (a `begin`/`and`/`or` operand list) with the
/// same fresh-id/rename discipline as `clone_fresh`. `None` if any element rejects.
pub(super) fn clone_vec(
    v: &[Hir],
    renames: &mut FxHashMap<Binding, Binding>,
    arena: &mut BindingArena,
) -> Option<Vec<Hir>> {
    v.iter().map(|c| clone_fresh(c, renames, arena)).collect()
}

/// Clone a function template with fresh parameter bindings (minted via `gensym`,
/// typed immutable-local, carrying the source parameter's `(numeric!)` declaration
/// — the proof a spliced raw `%`-intrinsic in the body rests on) and a fresh-id
/// body. Returns the fresh parameters and the cloned body, ready to splice like a
/// moved-out lambda's.
pub(super) fn clone_template(t: &FnTemplate, arena: &mut BindingArena) -> (Vec<Binding>, Hir) {
    let mut renames: FxHashMap<Binding, Binding> = FxHashMap::default();
    let mut params = Vec::with_capacity(t.params.len());
    for &p in &t.params {
        let declared_numeric = arena.get(p).declared_numeric;
        let fresh = arena.gensym();
        let fi = arena.get_mut(fresh);
        fi.is_immutable = true;
        fi.declared_numeric = declared_numeric;
        renames.insert(p, fresh);
        params.push(fresh);
    }
    let body = clone_fresh(&t.body, &mut renames, arena)
        .expect("collect_inline_fns proved the body inlineable");
    (params, body)
}

/// Pre-order walk: try to fuse a HOF chain rooted at `hir` (consuming the whole
/// chain, including its inner HOF calls); whether or not it fused, recurse into
/// the resulting node's children (which fuses nested HOFs in the spliced lambda
/// bodies or the base array's elements). A chain of any `map`/`filter` mix under
/// an optional outermost scalar terminal, over the same proven base, fuses to one
/// loop; the recursion still reaches HOFs nested inside a spliced lambda body or a
/// declined chain's inner run (including a fold whose composition was declined, or
/// a search — which takes no prefix at all — whose map/filter prefix then fuses on
/// its own).
pub(super) fn rewrite(
    hir: &mut Hir,
    arena: &mut BindingArena,
    symbol_names: &HashMap<u32, String>,
    ops: &Ops,
    bases: &FxHashMap<Binding, &'static str>,
    fns: &FnResolver,
) {
    if let Some(plan) = validate_chain(hir, arena, symbol_names, bases, fns) {
        let sig = hir.signal;
        let span = hir.span.clone();
        let owned = std::mem::replace(hir, Hir::error(span.clone()));
        let (terminal, stages, base) = take_chain(owned, plan, arena, fns);
        *hir = build_loop(terminal, stages, base, arena, ops, sig, span);
    }
    hir.for_each_child_mut(|c| rewrite(c, arena, symbol_names, ops, bases, fns));
}
