use super::*;
use crate::hir::fragment::HirFragment;

/// A function eligible for inlining into a fused HOF, held as an `HirFragment`
/// — a body closed over its own bindings ([`crate::hir::fragment`]).
///
/// The definition persists: it stays bound and may be used as a first-class
/// value, so its body cannot be moved out the way a call-site literal's is.
/// Each call site grafts a copy instead, minting the parameters and any `let`
/// bindings fresh in the consuming arena.
///
/// Everything a call site asks of a function is here, because a fragment's
/// defining arena is not readable where the graft happens — and, once the
/// fragment reaches another unit or another process, does not exist at all.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub(crate) struct FnFragment {
    /// Fragment indices of the parameters, in order. A graft's binding map is
    /// indexed by these to get the host bindings the splice binds elements to.
    pub(super) params: Vec<u32>,
    pub(super) fragment: HirFragment,
    /// Does the body prove it returns an **array**? Asked only of a `mapcat`,
    /// whose fused inner walk is indexed — linear over an array, quadratic over
    /// a list (docs/impl/dissolution.md § "Mapcat — the stage that fans out").
    /// Recorded here because the proof reads the defining unit's arena and
    /// init-keyword map, which the call site may not have.
    pub(super) returns_array: bool,
}

impl FnFragment {
    pub(super) fn arity(&self) -> usize {
        self.params.len()
    }

    /// The body's signal, fed to the composition-reorder gate.
    pub(super) fn signal(&self) -> Signal {
        self.fragment.body_signal()
    }

    /// Graft a copy into `arena`: fresh parameters and `let` bindings, free
    /// globals resolved by name. `None` if a global does not resolve here.
    ///
    /// Every minted binding is re-kinded as a compiler temporary, because the
    /// splice places it somewhere the defining unit did not. A parameter becomes
    /// a `let`-bound loop local — bound once per element by the emitted loop, not
    /// by a call — and the body's own `let` bindings are re-minted per call site,
    /// so neither answers to a name the user wrote any more. The facts that ARE
    /// about the binding rather than its home — its mutability, its `(numeric!)`
    /// floor — travel in the fragment and stand.
    pub(super) fn graft(
        &self,
        arena: &mut BindingArena,
        prim_by_name: &FxHashMap<SymbolId, Binding>,
    ) -> Option<(Vec<Binding>, Hir)> {
        let (map, body) = self.fragment.graft(arena, prim_by_name)?;
        for i in self.fragment.local_indices() {
            let inner = arena.get_mut(map[i as usize]);
            inner.name = SymbolId::SYNTHETIC;
            inner.scope = BindingScope::Local;
            inner.is_synthetic = true;
        }
        Some((self.params.iter().map(|&i| map[i as usize]).collect(), body))
    }
}

/// Collects this unit's inlineable functions into both maps a call site
/// consults.
pub(super) struct Collector<'a> {
    arena: &'a BindingArena,
    bases: &'a FxHashMap<Binding, &'static str>,
    /// By the `Binding` this unit knows the function by — how a same-unit `Var`
    /// argument finds it.
    pub(super) templates: FxHashMap<Binding, FnFragment>,
    /// A binding bound more than once has no single stable value, so it leaves
    /// the by-binding map. The registry keeps first-wins instead: a name is
    /// claimed by the unit that defines it, and a later same-named user binding
    /// must not clobber the stdlib's.
    seen: FxHashSet<Binding>,
}

impl<'a> Collector<'a> {
    pub(super) fn new(
        arena: &'a BindingArena,
        bases: &'a FxHashMap<Binding, &'static str>,
    ) -> Self {
        Collector {
            arena,
            bases,
            templates: FxHashMap::default(),
            seen: FxHashSet::default(),
        }
    }

    /// Walk every `Let`/`Letrec`/`Define` binding — the forms
    /// `prune::collect_inits` visits — recording each inlineable function under
    /// its binding here and under its name in `registry`, for every later unit.
    /// One gate serves both: a fragment either closes or it does not.
    pub(super) fn walk(&mut self, hir: &Hir, registry: &mut FnInlineRegistry) {
        match &hir.kind {
            HirKind::Let { bindings, .. } | HirKind::Letrec { bindings, .. } => {
                for (b, value) in bindings {
                    self.record(*b, value, registry);
                }
            }
            HirKind::Define { binding, value } => self.record(*binding, value, registry),
            _ => {}
        }
        hir.for_each_child(|c| self.walk(c, registry));
    }

    fn record(&mut self, b: Binding, value: &Hir, registry: &mut FnInlineRegistry) {
        let doubly_bound = !self.seen.insert(b);
        if doubly_bound {
            self.templates.remove(&b);
        }
        let inner = self.arena.get(b);
        if !inner.is_immutable || inner.is_mutated {
            return;
        }
        let Some(fragment) = fn_fragment(value, self.arena, self.bases) else {
            return;
        };
        registry.record(inner.name, fragment.clone());
        if !doubly_bound {
            self.templates.insert(b, fragment);
        }
    }
}

/// The inlineable fragment of a lambda initializer, or `None`.
///
/// The lambda must have 1 or 2 fixed parameters (a `map`/`filter`/`count`/search
/// element, or a `fold` accumulator and element — the use site checks the exact
/// arity), no rest parameter, and unmutated parameters. Its body must then close
/// over its own bindings, which is what rejects a body naming an enclosing
/// runtime local: such a name belongs to the scope the function was defined in,
/// which the call site need not sit inside — unlike a call-site literal, which
/// is spliced at its own scope and keeps its captures
/// (docs/impl/dissolution.md § "Captures").
fn fn_fragment(
    value: &Hir,
    arena: &BindingArena,
    bases: &FxHashMap<Binding, &'static str>,
) -> Option<FnFragment> {
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
    if params.iter().any(|p| arena.get(*p).is_mutated) {
        return None;
    }
    let (params, fragment) = HirFragment::close(body, params, arena, *assert_numeric)?;
    Some(FnFragment {
        params,
        fragment,
        returns_array: classify_base(body, arena, bases).is_some(),
    })
}
