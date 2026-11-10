// audited: 2026-11-10
//! The `def` forms a body owns: the first `def` of each name pre-bound, and the analysis of each `def` site.
//!
//! docs/bindings.md

use super::*;
use crate::syntax::Syntax;

mod reach;
mod site;
pub(super) use reach::{reach, Reached};

/// A `def` or `var` form, named by the address of its items. The walk and the
/// analysis reach the form through one syntax node, so the address names one
/// form while its body is analyzed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub(super) struct DefSite(*const Syntax);

impl DefSite {
    pub(super) fn of(items: &[Syntax]) -> Self {
        DefSite(items.as_ptr())
    }
}

/// A name as binding identity: its symbol and its sorted scope set.
#[derive(Clone, PartialEq, Eq, Hash, Debug)]
pub(super) struct NameKey {
    sym: SymbolId,
    scopes: Vec<ScopeId>,
}

impl NameKey {
    pub(super) fn new(sym: SymbolId, scopes: &[ScopeId]) -> Self {
        let mut scopes = scopes.to_vec();
        scopes.sort_unstable_by_key(|s| s.0);
        NameKey { sym, scopes }
    }
}

/// The bindings a body pre-bound for the first `def` of each name a `def`
/// form binds.
#[derive(Clone)]
pub(super) struct OwnedDef {
    names: Vec<(String, Binding)>,
    /// The index in `DefState::owners` of the body that pre-bound them.
    owner: usize,
}

impl OwnedDef {
    fn binding_of(&self, name: &str) -> Option<Binding> {
        self.names.iter().find(|(n, _)| n == name).map(|(_, b)| *b)
    }
}

/// A body whose `def` forms are being analyzed.
#[derive(Default)]
pub(super) struct Owner {
    /// Each pre-bound binding a lambda reads before its `def` runs, where the
    /// read needs a value: set at the body's start, assigned by its `def`.
    hoisted: Vec<(Binding, Hir)>,
}

/// The analyzer's state for the `def` forms of the bodies it is in.
#[derive(Default)]
pub(super) struct DefState {
    /// The pre-bound bindings a read resolved to before their `def`.
    pub(super) forward_reads: HashSet<Binding>,
    /// The merges made so far, and the reads that reached them (`merge.rs`).
    pub(super) uses: super::merge::MergeUses,
    /// The merged bindings the next expression analyzed must define first.
    pub(super) pending: Vec<super::merge::Pending>,
    /// Each merged binding and what it may stand for, in the order the merges
    /// closed (`merge/join.rs`).
    pub(super) joins: Vec<super::merge::Join>,
    /// The lambdas the fixpoint of the file being analyzed re-analyzes.
    pub(super) file: Option<super::fileletrec::FileLambdas>,
    pub(super) owners: Vec<Owner>,
    /// Whether a lookup read past a pending first `def`, for the message a
    /// name that resolves to nothing gets.
    pub(super) skipped_pending: bool,
}

impl<'a> Analyzer<'a> {
    /// The decision the current scope's body made for `site`, if it owns it.
    fn owned_def(&self, site: DefSite) -> Option<&OwnedDef> {
        self.scopes.last().and_then(|scope| scope.owned.get(&site))
    }

    /// The names a `def`'s target binds, with each name's scopes and whether
    /// an `@` marks it mutable. A target that is neither a symbol nor a
    /// pattern binds none, and the analysis of the `def` reports it.
    pub(super) fn def_names(target: &Syntax) -> Vec<(&str, &[ScopeId], bool)> {
        if let Some(raw) = target.as_symbol() {
            let (name, at_mutable) = super::strip_at_prefix(raw);
            return vec![(name, target.scopes.as_slice(), at_mutable)];
        }
        let mut leaves = Vec::new();
        if Self::is_destructure_pattern(target) {
            Self::extract_pattern_leaves(target, &mut leaves);
        }
        leaves
    }

    /// Whether a name a `def` form binds is mutable: a `var`, or an `@` name.
    pub(super) fn def_is_mutable(
        &self,
        form_is_def: bool,
        pattern: bool,
        at_mutable: bool,
    ) -> bool {
        let immutable = form_is_def && (pattern || self.immutable_by_default);
        !(immutable && !at_mutable)
    }

    /// Record that a `def` in the current scope binds `key` with the given
    /// mutability. Every `def` of a name in one scope agrees.
    pub(super) fn declare(
        &mut self,
        key: &NameKey,
        name: &str,
        mutable: bool,
        span: &Span,
    ) -> Result<(), String> {
        let Some(scope) = self.scopes.last_mut() else {
            return Ok(());
        };
        match scope.declared.get(key) {
            Some(&(first, at)) if first != mutable => Err(format!(
                "{}: '{}' is defined both mutable and immutable in one scope (first at {})",
                span, name, at
            )),
            Some(_) => Ok(()),
            None => {
                scope.declared.insert(key.clone(), (mutable, *span));
                Ok(())
            }
        }
    }

    /// Note a read that resolved to a pre-bound binding before its `def`. A
    /// lambda may capture it before it holds a value, so it takes a cell. The
    /// `def`'s own value does not count: it cannot run before the `def`.
    pub(super) fn read_forward(&mut self, binding: Binding) {
        if self.current_init_binding != Some(binding) {
            self.defs.forward_reads.insert(binding);
            self.arena.get_mut(binding).is_prebound = true;
        }
    }

    /// Pre-bind the first `def` of each name `items` reach, in a scope that is
    /// a body. The file binds its own (`fileletrec`), and the global scope
    /// binds each `def` at its site.
    fn own_body(&mut self, items: &[Syntax], owner: usize) -> Result<(), String> {
        if self.scopes.last().is_none_or(|s| s.definition_env) {
            return Ok(());
        }
        let mut reached = Vec::new();
        for item in items {
            reach(item, true, &mut reached);
        }
        self.forward_defs(reached, owner).map(drop)
    }

    /// Declare each name the `def` forms in `reached` bind in the current
    /// scope, and pre-bind the first `def` of each name for the body `owner`.
    /// Answers the names it pre-bound.
    pub(super) fn forward_defs(
        &mut self,
        reached: Vec<Reached>,
        owner: usize,
    ) -> Result<Vec<String>, String> {
        let mut forwarded = Vec::new();
        for Reached {
            items: def,
            straight,
        } in reached
        {
            let form_is_def = def[0].as_symbol() == Some("def");
            let pattern = Self::is_destructure_pattern(&def[1]);
            let mut names = Vec::new();
            for (name, scopes, at_mutable) in Self::def_names(&def[1]) {
                let key = NameKey::new(self.symbols.intern(name), scopes);
                let first = self
                    .scopes
                    .last()
                    .is_some_and(|s| !s.declared.contains_key(&key));
                let mutable = self.def_is_mutable(form_is_def, pattern, at_mutable);
                self.declare(&key, name, mutable, &def[1].span)?;
                if !first {
                    continue;
                }
                let binding = self.arena.alloc(key.sym, BindingScope::Local);
                self.register_forward(name, scopes, binding);
                let fn_depth = self.fn_depth;
                let inner = self.arena.get_mut(binding);
                inner.prebind_fn_depth = fn_depth;
                // A lambda before it may capture it, as a run of local `defn`
                // forms does.
                inner.is_prebound = straight;
                names.push((name.to_string(), binding));
                forwarded.push(name.to_string());
            }
            if !names.is_empty() {
                let site = DefSite::of(def);
                if let Some(scope) = self.scopes.last_mut() {
                    scope.owned.insert(site, OwnedDef { names, owner });
                }
            }
        }
        Ok(forwarded)
    }

    /// Declare each name the top-level `def` or `var` of `target` binds.
    pub(super) fn declare_targets(
        &mut self,
        target: &Syntax,
        form_is_def: bool,
    ) -> Result<(), String> {
        let pattern = Self::is_destructure_pattern(target);
        for (name, scopes, at_mutable) in Self::def_names(target) {
            let key = NameKey::new(self.symbols.intern(name), scopes);
            let mutable = self.def_is_mutable(form_is_def, pattern, at_mutable);
            self.declare(&key, name, mutable, &target.span)?;
        }
        Ok(())
    }

    /// Analyze `items` in order as a body that owns its `def` forms. Answers
    /// the nodes and their combined signal; a pre-bound binding a lambda reads
    /// before its `def` is set first.
    pub(super) fn analyze_owned_body(
        &mut self,
        items: &[Syntax],
    ) -> Result<(Vec<Hir>, Signal), String> {
        self.defs.owners.push(Owner::default());
        let owner = self.defs.owners.len() - 1;
        let analyzed = self.own_body(items, owner).and_then(|()| {
            let mut exprs = Vec::with_capacity(items.len());
            let mut signal = Signal::silent();
            for item in items {
                let hir = self.analyze_expr(item)?;
                signal = signal.combine(hir.signal);
                exprs.push(hir);
            }
            Ok((exprs, signal))
        });
        let frame = self.defs.owners.pop().unwrap_or_default();
        let (body, signal) = analyzed?;
        let mut exprs = self.settle_hoisted(frame);
        exprs.extend(body);
        Ok((exprs, signal))
    }
}
