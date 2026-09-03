use super::*;

/// Per-instance persistent map of cross-unit-inlineable function fragments,
/// keyed by function NAME (`SymbolId`). Each unit's `fuse_map_chains` records
/// its locally-defined inlineable functions here (the `<stdlib>` compile records
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
///
/// A `FnFragment` is closed over its own bindings, so the registry is plain data:
/// it snapshots to the stdlib disk cache and restores whole, and a cache hit
/// inlines the set a stdlib compile records rather than a subset of it.
#[derive(Default)]
pub struct FnInlineRegistry {
    pub(crate) by_name: FxHashMap<SymbolId, FnFragment>,
}

impl FnInlineRegistry {
    /// Record a fragment under its name. First definition wins, so the stdlib's
    /// canonical fn is never clobbered by a later same-named user binding, and
    /// re-recording across compiles is a cheap no-op.
    pub(super) fn record(&mut self, name: SymbolId, f: FnFragment) {
        self.by_name.entry(name).or_insert(f);
    }

    /// Snapshot the registry for the stdlib disk cache. Ids are name hashes and
    /// cross unchanged (docs/impl/symbol.md); the spellings ride alongside so
    /// the loading instance can re-intern them into its own display memo.
    pub(crate) fn to_stored(&self, symbols: &crate::symbol::SymbolTable) -> StoredFnInlineRegistry {
        StoredFnInlineRegistry {
            by_name: self
                .by_name
                .iter()
                .map(|(name, f)| (symbols.name(*name).unwrap_or("").to_string(), f.clone()))
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
        for (name, f) in stored.by_name {
            self.by_name.insert(symbols.intern(&name), f);
        }
    }
}

/// The resolution context for a HOF's function argument: same-unit fragments
/// (matched by `Binding`) and cross-unit fragments (matched by the callee's
/// primitive NAME through the persistent registry). A lambda literal, a
/// same-unit `Var`, and a cross-unit stdlib `Var` all resolve through here.
pub(super) struct FnResolver<'a> {
    pub(super) templates: &'a FxHashMap<Binding, FnFragment>,
    pub(super) registry: &'a FnInlineRegistry,
    /// This unit's primitives by name, which every fragment's free globals
    /// resolve through.
    pub(super) prim_by_name: &'a FxHashMap<SymbolId, Binding>,
}

impl<'a> FnResolver<'a> {
    /// The fragment a `Var` argument names and can be grafted from here: this
    /// unit's own, or — for an `is_primitive` callee, a `bind_primitives` stdlib
    /// export — the cross-unit registry's. A user redefinition shadows the
    /// export with a non-primitive binding and is left alone.
    ///
    /// `None` when no fragment matches or a free global does not resolve in this
    /// unit. That last check is what makes [`Self::take_parts`]'s graft total.
    ///
    /// The result borrows the maps, never `arena`, so a caller may go on to
    /// borrow the arena mutably for the graft.
    fn fragment(&self, b: Binding, arena: &BindingArena) -> Option<&'a FnFragment> {
        let f = match self.templates.get(&b) {
            Some(f) => f,
            None => {
                let inner = arena.get(b);
                if !inner.is_primitive {
                    return None;
                }
                self.registry.by_name.get(&inner.name)?
            }
        };
        f.fragment.globals_resolve(self.prim_by_name).then_some(f)
    }

    /// The body signal of a HOF's function argument at the given arity, or
    /// `None` if it does not qualify — fed to the reorder gate (all forms gated
    /// identically).
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
                let f = self.fragment(*b, arena)?;
                (f.arity() == arity).then(|| f.signal())
            }
            _ => None,
        }
    }

    /// Does this function argument's body prove it returns an **array**? Asked
    /// only of a `mapcat`, the one op that reads its function's result as a
    /// collection and walks it: the fused inner walk is an indexed one, linear
    /// over an array and quadratic over a list (docs/impl/dissolution.md
    /// § "Mapcat — the stage that fans out").
    ///
    /// A call-site literal is read here, against this unit's own init-keyword
    /// proof. A fragment answers from the fact it recorded when it closed, which
    /// is the same proof run in the unit that could see the body's bindings.
    pub(super) fn result_is_array(
        &self,
        lam: &Hir,
        arena: &BindingArena,
        bases: &FxHashMap<Binding, &'static str>,
    ) -> bool {
        match &lam.kind {
            HirKind::Lambda { body, .. } => classify_base(body, arena, bases).is_some(),
            HirKind::Var(b) => self.fragment(*b, arena).is_some_and(|f| f.returns_array),
            _ => false,
        }
    }

    /// Resolve a HOF's function argument to owned `(params, body)`, ready to
    /// splice: a **lambda literal** is *moved* out; a `Var` grafts its fragment,
    /// minting fresh parameters and `let` bindings in this arena and resolving
    /// its free globals by name. `body_signal` proved one path holds at the
    /// required arity and that the globals resolve, so the resolution is total.
    pub(super) fn take_parts(&self, lam: Hir, arena: &mut BindingArena) -> (Vec<Binding>, Hir) {
        match lam.kind {
            HirKind::Lambda { params, body, .. } => (params, *body),
            HirKind::Var(b) => {
                let f = self
                    .fragment(b, arena)
                    .expect("validate_chain proved a graftable fragment");
                f.graft(arena, self.prim_by_name)
                    .expect("validate_chain proved the free globals resolve")
            }
            _ => unreachable!("validate_chain proved a lambda or a fragment Var"),
        }
    }
}

/// Serializable snapshot of [`FnInlineRegistry`] for the stdlib disk cache.
#[derive(serde::Serialize, serde::Deserialize, Default)]
pub(crate) struct StoredFnInlineRegistry {
    pub(crate) by_name: Vec<(String, FnFragment)>,
}
