use super::*;

/// A same-unit function template summarized for the cross-unit registry: its
/// arity, its (defining-unit) parameters and body, its free globals recorded by
/// `SymbolId`, and its body's top signal. A `Binding` is a per-arena index,
/// meaningless in a later unit; the clone freshens the parameters and any
/// `let`-bound bindings per call site, and re-resolves every free global by name
/// against the consuming unit's own primitive bindings (`clone_reg_template`). The
/// signal feeds the composition-reorder gate exactly as a same-unit template's does.
pub(crate) struct RegFnTemplate {
    pub(super) arity: usize,
    pub(super) params: Vec<Binding>,
    pub(super) body: Hir,
    pub(super) globals: Vec<(Binding, SymbolId)>,
    pub(super) signal: Signal,
    /// The defining function's `(numeric!)` declaration, replayed onto the fresh
    /// parameters at each call site (a `BindingInner` flag cannot be read across
    /// arenas, so the fact travels here). It is what proves a spliced raw
    /// `%`-intrinsic in the body — see `BindingInner::declared_numeric`.
    pub(super) declared_numeric: bool,
}
impl RegFnTemplate {
    /// `clone_fresh` re-mints `let`-bound bindings by reading their metadata off
    /// the defining arena (`arena.get`); a template restored from the stdlib
    /// disk cache has no such arena, so templates whose body contains a `let`
    /// are not serialized (they stay un-inlined on the cached path — a
    /// performance-only difference). Pure-expression bodies are safe.
    pub(super) fn body_has_let(h: &Hir) -> bool {
        match &h.kind {
            HirKind::Let { .. } => true,
            _ => {
                let mut found = false;
                h.for_each_child(|c| {
                    if !found && Self::body_has_let(c) {
                        found = true;
                    }
                });
                found
            }
        }
    }
}

/// Per-instance persistent map of cross-unit-inlineable function templates, keyed
/// by function NAME (`SymbolId`). Each unit's `fuse_map_chains` records its
/// locally-defined inlineable functions here (the `<stdlib>` compile records
/// `inc`/`dec`/…), and every later unit consults it, so a user→stdlib `(map inc
/// xs)` inlines the stdlib body exactly as a same-unit named fn does — the
/// dissolution leg reaching across the compile-unit boundary
/// (docs/impl/dissolution.md § "Cross-unit named functions").
///
/// Compile-time-only state: the rewrite it drives leaves the inlined body in the
/// HIR, so nothing here reaches the runtime. It rides on `CompileCtx` (the
/// per-instance compile context) precisely because it must outlive the single
/// compile that defined the function — never on any VM/region structure. The
/// pattern mirrors `monomorphize::DispatchWrapperRegistry`.
#[derive(Default)]
pub struct FnInlineRegistry {
    pub(crate) by_name: FxHashMap<SymbolId, RegFnTemplate>,
}

impl FnInlineRegistry {
    /// Record a locally-collected template under its name. First definition wins,
    /// so the stdlib's canonical fn is never clobbered by a later same-named user
    /// binding, and re-recording across compiles is a cheap no-op.
    pub(super) fn record(&mut self, name: SymbolId, t: RegFnTemplate) {
        self.by_name.entry(name).or_insert(t);
    }
    /// Snapshot the registry for the stdlib disk cache. Templates whose body
    /// contains a `let` are skipped (`clone_fresh` re-mints their bindings off
    /// the defining arena, which a reloaded template cannot provide). Names
    /// travel instead of per-process `SymbolId`s.
    pub(crate) fn to_stored(&self, symbols: &crate::symbol::SymbolTable) -> StoredFnInlineRegistry {
        StoredFnInlineRegistry {
            by_name: self
                .by_name
                .iter()
                .filter(|(_, t)| !RegFnTemplate::body_has_let(&t.body))
                .map(|(name, t)| {
                    (
                        symbols.name(*name).unwrap_or("").to_string(),
                        StoredFnTemplate {
                            arity: t.arity,
                            params: t.params.iter().map(|b| b.0).collect(),
                            body: t.body.clone(),
                            globals: t
                                .globals
                                .iter()
                                .map(|(b, s)| (b.0, symbols.name(*s).unwrap_or("").to_string()))
                                .collect(),
                            signal: t.signal,
                            declared_numeric: t.declared_numeric,
                        },
                    )
                })
                .collect(),
        }
    }
    /// Restore a snapshot into this registry (stdlib disk cache load path);
    /// re-interns names in the loading process's table.
    pub(crate) fn restore(
        &mut self,
        stored: StoredFnInlineRegistry,
        symbols: &mut crate::symbol::SymbolTable,
    ) {
        self.by_name.clear();
        for (name, t) in stored.by_name {
            self.by_name.insert(
                symbols.intern(&name),
                RegFnTemplate {
                    arity: t.arity,
                    params: t.params.iter().map(|&i| Binding(i)).collect(),
                    body: t.body,
                    globals: t
                        .globals
                        .iter()
                        .map(|(i, n)| (Binding(*i), symbols.intern(n)))
                        .collect(),
                    signal: t.signal,
                    declared_numeric: t.declared_numeric,
                },
            );
        }
    }
}

/// Walk every `Let`/`Letrec`/`Define` binding (the forms `collect_inline_fns`
/// visits) and record each cross-unit-inlineable function into `registry` by name.
/// Unlike the same-unit collector, this admits a lambda that references module
/// globals — a stdlib `defn` whose siblings are `is_file_scope` letrec bindings not
/// yet `is_primitive` (`cross_unit_fn_template`) — recording those globals by name.
pub(super) fn record_cross_unit_fns(
    hir: &Hir,
    arena: &BindingArena,
    registry: &mut FnInlineRegistry,
) {
    let record = |b: Binding, value: &Hir, registry: &mut FnInlineRegistry| {
        let bi = arena.get(b);
        if !bi.is_immutable || bi.is_mutated {
            return;
        }
        if let Some(t) = cross_unit_fn_template(value, arena) {
            registry.record(bi.name, t);
        }
    };
    match &hir.kind {
        HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
            for (b, value) in bindings {
                record(*b, value, registry);
            }
        }
        HirKind::Define { binding, value } => record(*binding, value, registry),
        _ => {}
    }
    hir.for_each_child(|c| record_cross_unit_fns(c, arena, registry));
}

/// The cross-unit inlineable template of a lambda initializer, or `None`. Like
/// `fn_template`, but where the same-unit gate requires **no captures at all**,
/// this admits a lambda whose every free variable is a genuine **global** — an
/// `is_file_scope` module name (a stdlib `defn`'s sibling reference, not yet
/// `is_primitive` during the stdlib compile) or an `is_primitive` binding — and
/// records those globals by name for re-resolution (`collect_globals`). A free
/// variable that is a plain enclosing local (a real capture of a runtime value)
/// disqualifies it, exactly as the same-unit gate intends.
pub(super) fn cross_unit_fn_template(value: &Hir, arena: &BindingArena) -> Option<RegFnTemplate> {
    let HirKind::Lambda {
        params,
        rest_param,
        body,
        assert_numeric,
        ..
    } = &value.kind
    else {
        return None;
    };
    if rest_param.is_some() || params.is_empty() || params.len() > 2 {
        return None;
    }
    if params.iter().any(|p| arena.get(*p).is_mutated) || !is_inlineable_body(body, *assert_numeric)
    {
        return None;
    }
    let globals = collect_globals(body, params, arena)?;
    Some(RegFnTemplate {
        arity: params.len(),
        params: params.clone(),
        body: (**body).clone(),
        globals,
        signal: body.signal,
        declared_numeric: *assert_numeric,
    })
}

/// Collect a body's free globals — every `Var` that is neither a parameter nor a
/// `let`-binding the body introduces — as deduplicated `(binding, name)` pairs.
/// Returns `None` if any free variable is NOT a genuine global (an `is_file_scope`
/// module name or an `is_primitive` binding): such a variable is a real capture of
/// an enclosing runtime local, which cannot be re-resolved by name in a consuming
/// unit, so the function is not cross-unit inlineable. `let` is the only
/// binding-introducing form in the clone whitelist, so only its bindings extend the
/// bound set.
pub(super) fn collect_globals(
    body: &Hir,
    params: &[Binding],
    arena: &BindingArena,
) -> Option<Vec<(Binding, SymbolId)>> {
    let mut bound: FxHashSet<Binding> = params.iter().copied().collect();
    let mut out: Vec<(Binding, SymbolId)> = Vec::new();
    let mut seen: FxHashSet<Binding> = FxHashSet::default();
    walk_globals(body, &mut bound, &mut out, &mut seen, arena).then_some(out)
}

/// The traversal behind `collect_globals`; returns `false` the moment a free
/// variable is a non-global local (an unclonable capture).
pub(super) fn walk_globals(
    h: &Hir,
    bound: &mut FxHashSet<Binding>,
    out: &mut Vec<(Binding, SymbolId)>,
    seen: &mut FxHashSet<Binding>,
    arena: &BindingArena,
) -> bool {
    match &h.kind {
        HirKind::Var(b) => {
            if bound.contains(b) {
                return true;
            }
            let bi = arena.get(*b);
            if !bi.is_file_scope && !bi.is_primitive {
                return false;
            }
            if seen.insert(*b) {
                out.push((*b, bi.name));
            }
            true
        }
        HirKind::Let { bindings, body } => {
            for (b, value) in bindings {
                if !walk_globals(value, bound, out, seen, arena) {
                    return false;
                }
                bound.insert(*b);
            }
            walk_globals(body, bound, out, seen, arena)
        }
        _ => {
            let mut ok = true;
            h.for_each_child(|c| {
                if ok {
                    ok = walk_globals(c, bound, out, seen, arena);
                }
            });
            ok
        }
    }
}

/// Clone a cross-unit template with fresh parameter/`let` bindings and its free
/// globals re-resolved by name to this unit's primitive bindings, ready to splice
/// like a moved-out lambda's. Seeds the rename map with the global remaps first —
/// so the shared `clone_fresh` rewrites each free-global `Var` to the consuming
/// unit's binding with no further machinery — then freshens the parameters exactly
/// as `clone_template` does. `None` if any free global does not resolve to a
/// primitive here (the arg declines; `FnResolver::body_signal` proves this cannot
/// happen once a chain is validated).
pub(super) fn clone_reg_template(
    t: &RegFnTemplate,
    arena: &mut BindingArena,
    prim_by_name: &FxHashMap<SymbolId, Binding>,
) -> Option<(Vec<Binding>, Hir)> {
    let mut renames: FxHashMap<Binding, Binding> = FxHashMap::default();
    for (old, name) in &t.globals {
        renames.insert(*old, *prim_by_name.get(name)?);
    }
    let mut params = Vec::with_capacity(t.params.len());
    for &p in &t.params {
        let fresh = arena.gensym();
        let fi = arena.get_mut(fresh);
        fi.is_immutable = true;
        fi.declared_numeric = t.declared_numeric;
        renames.insert(p, fresh);
        params.push(fresh);
    }
    let body = clone_fresh(&t.body, &mut renames, arena)?;
    Some((params, body))
}

/// The resolution context for a HOF's function argument: same-unit templates
/// (matched by `Binding`, spliced within this unit) and cross-unit templates
/// (matched by the callee's primitive NAME through the persistent registry, with
/// free globals re-resolved through `prim_by_name`). A lambda literal, a same-unit
/// `Var`, and a cross-unit stdlib `Var` all resolve through here.
pub(super) struct FnResolver<'a> {
    pub(super) templates: &'a FxHashMap<Binding, FnTemplate>,
    pub(super) registry: &'a FnInlineRegistry,
    pub(super) prim_by_name: &'a FxHashMap<SymbolId, Binding>,
}

impl FnResolver<'_> {
    /// The body signal of a HOF's function argument at the given arity, or `None`
    /// if it does not qualify — fed to the reorder gate (all forms gated
    /// identically). A cross-unit `Var` (a `Var` naming no same-unit template but
    /// bound to an `is_primitive` stdlib export) qualifies only when the registry
    /// holds a matching-arity template whose every free global resolves in THIS
    /// unit, so a validated chain is always one `take_parts` can then clone.
    pub(super) fn body_signal(
        &self,
        lam: &Hir,
        arena: &BindingArena,
        arity: usize,
    ) -> Option<Signal> {
        match &lam.kind {
            HirKind::Lambda { .. } => {
                qualifies_lambda(lam, arena, arity).map(|(_, body)| body.signal)
            }
            HirKind::Var(b) => {
                if let Some(t) = self.templates.get(b) {
                    (t.params.len() == arity).then_some(t.body.signal)
                } else if arena.get(*b).is_primitive {
                    let t = self.registry.by_name.get(&arena.get(*b).name)?;
                    if t.arity != arity
                        || t.globals
                            .iter()
                            .any(|(_, n)| !self.prim_by_name.contains_key(n))
                    {
                        return None;
                    }
                    Some(t.signal)
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Does this function argument's body prove it returns an **array**? Asked only
    /// of a `mapcat`, the one op that reads its function's result as a collection and
    /// walks it: the fused inner walk is an indexed one, linear over an array and
    /// quadratic over a list (docs/impl/dissolution.md § "Mapcat — the stage that
    /// fans out").
    ///
    /// The body is read by `classify_base`, the same proof the chain's own base
    /// collection is read by, so it answers for a call-site array producer and for a
    /// `Var` alias of one. A **cross-unit** template declines: its body names the
    /// defining unit's bindings, which neither this arena nor this unit's init-keyword
    /// map can resolve, so no reading of it here would be sound.
    pub(super) fn result_is_array(
        &self,
        lam: &Hir,
        arena: &BindingArena,
        symbol_names: &HashMap<u32, String>,
        bases: &FxHashMap<Binding, &'static str>,
    ) -> bool {
        let body = match &lam.kind {
            HirKind::Lambda { body, .. } => &**body,
            HirKind::Var(b) => match self.templates.get(b) {
                Some(t) => &t.body,
                None => return false,
            },
            _ => return false,
        };
        classify_base(body, arena, symbol_names, bases).is_some()
    }

    /// Resolve a HOF's function argument to owned `(params, body)`, ready to splice:
    /// a **lambda literal** is *moved* out; a same-unit `Var` *clones* its template;
    /// a cross-unit stdlib `Var` clones the registry template with its globals
    /// re-resolved by name. `body_signal` proved one path holds at the required
    /// arity (and, cross-unit, that the globals resolve), so the resolution is total.
    pub(super) fn take_parts(&self, lam: Hir, arena: &mut BindingArena) -> (Vec<Binding>, Hir) {
        match lam.kind {
            HirKind::Lambda { params, body, .. } => (params, *body),
            HirKind::Var(b) => {
                if let Some(t) = self.templates.get(&b) {
                    clone_template(t, arena)
                } else {
                    let t = self
                        .registry
                        .by_name
                        .get(&arena.get(b).name)
                        .expect("validate_chain proved a cross-unit template");
                    clone_reg_template(t, arena, self.prim_by_name)
                        .expect("validate_chain proved the free globals resolve")
                }
            }
            _ => unreachable!("validate_chain proved a lambda or a template Var"),
        }
    }
}

/// Serializable snapshot of [`FnInlineRegistry`] for the stdlib disk cache.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct StoredFnInlineRegistry {
    pub(crate) by_name: Vec<(String, StoredFnTemplate)>,
}
#[derive(serde::Serialize, serde::Deserialize)]
pub(crate) struct StoredFnTemplate {
    pub(crate) arity: usize,
    pub(crate) params: Vec<u32>,
    pub(crate) body: Hir,
    pub(crate) globals: Vec<(u32, String)>,
    pub(crate) signal: Signal,
    pub(crate) declared_numeric: bool,
}
