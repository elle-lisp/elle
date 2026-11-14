// audited: 2026-11-14
//! The analysis of one `def` or `var` site: a new binding of each name, or the one its body pre-bound and may hoist.
//!
//! docs/bindings.md

use super::super::*;
use super::{DefSite, NameKey, Owner};
use crate::syntax::Syntax;

/// How a name a `def` site binds enters its scope.
#[derive(Clone, Copy)]
enum Entry {
    /// The body that owns the site pre-bound it as a `Forward` entry.
    Owned(usize),
    /// It is visible from before its value, so its lambda can call itself.
    Registered,
    /// It is a `Defining` entry while its value is analyzed.
    Defining,
}

/// One name a `def` site binds, while the site is analyzed.
struct DefSiteName {
    key: NameKey,
    name: String,
    scopes: Vec<ScopeId>,
    binding: Binding,
    entry: Entry,
    /// It holds a value before the `def` runs, so the `def` assigns it.
    assigned: bool,
}

impl<'a> Analyzer<'a> {
    /// Whether the pre-bound `binding` a `def` initializes must hold a value
    /// before the `def` runs: a lambda read it first, and either the `def` may
    /// be skipped, a binding outside the body gives it a value, or it stands
    /// at file level. Hoists it to its owner with that value, nil if none.
    fn hoist(
        &mut self,
        binding: Binding,
        name: &str,
        scopes: &[ScopeId],
        owner: usize,
        span: Span,
    ) -> bool {
        if !self.defs.forward_reads.contains(&binding) {
            return false;
        }
        let Some(scope) = self.scopes.last() else {
            return false;
        };
        let file = scope.definition_env;
        let in_merge = !scope.merges.is_empty();
        let outer = if file {
            None
        } else {
            self.lookup_outside(name, scopes)
        };
        if !(file || in_merge || outer.is_some()) {
            return false;
        }
        let init = match outer {
            Some(outer) => Hir::silent(HirKind::Var(outer), span),
            None => Hir::silent(HirKind::Nil, span),
        };
        if let Some(frame) = self.defs.owners.get_mut(owner) {
            frame.hoisted.push((binding, init));
        }
        self.unseed(binding);
        true
    }

    /// The value the binding of a later `def` holds before the `def` runs,
    /// where a lambda in its value read it: the binding before it, or nil. Its
    /// `def` then assigns it. Call it while the entry is `Defining`.
    pub(in crate::hir::analyze) fn preset(
        &mut self,
        binding: Binding,
        name: &str,
        scopes: &[ScopeId],
        span: Span,
    ) -> Result<Option<Hir>, String> {
        if !self.defs.forward_reads.contains(&binding) {
            return Ok(None);
        }
        let init = match self.lookup(name, scopes) {
            Some(earlier) => {
                self.check_merge_read(earlier, name, &span)?;
                Hir::silent(HirKind::Var(earlier), span)
            }
            None => Hir::silent(HirKind::Nil, span),
        };
        self.unseed(binding);
        self.arena.get_mut(binding).is_immutable = false;
        Ok(Some(init))
    }

    /// Forget what a seed promised of `binding`, which holds another value
    /// before its `def` runs, and make it a var its `def` assigns.
    fn unseed(&mut self, binding: Binding) {
        self.signal_env.remove(&binding);
        self.arity_env.remove(&binding);
        self.keyword_collectors.remove(&binding);
        let inner = self.arena.get_mut(binding);
        inner.is_mutated = true;
        inner.is_prebound = true;
    }

    /// The `Define` of each binding `owner` hoisted, now that its body is
    /// analyzed.
    pub(in crate::hir::analyze) fn settle_hoisted(&mut self, mut owner: Owner) -> Vec<Hir> {
        self.take_hoisted(&mut owner)
            .into_iter()
            .map(|(binding, init)| {
                let span = init.span;
                Hir::silent(
                    HirKind::Define {
                        binding,
                        value: Box::new(init),
                    },
                    span,
                )
            })
            .collect()
    }

    /// Each binding `owner` hoisted so far, with the value it starts from.
    /// Each one is a var from here on: its `def` assigns it.
    pub(in crate::hir::analyze) fn take_hoisted(
        &mut self,
        owner: &mut Owner,
    ) -> Vec<(Binding, Hir)> {
        let hoisted = std::mem::take(&mut owner.hoisted);
        for (binding, _) in &hoisted {
            self.arena.get_mut(*binding).is_immutable = false;
        }
        hoisted
    }

    /// The bindings of `sites` a lambda in the `def`'s value read, each now
    /// set before the `def` runs: hoisted to its owner, or preset in place.
    /// Answers the `Define` of each preset.
    fn settle_value_reads(
        &mut self,
        sites: &mut [DefSiteName],
        span: Span,
    ) -> Result<Vec<Hir>, String> {
        let mut presets = Vec::new();
        for site in sites.iter_mut().filter(|s| !s.assigned) {
            match site.entry {
                Entry::Owned(owner) => {
                    site.assigned = self.hoist(site.binding, &site.name, &site.scopes, owner, span);
                }
                Entry::Defining => {
                    if let Some(init) = self.preset(site.binding, &site.name, &site.scopes, span)? {
                        site.assigned = true;
                        let value = Box::new(init);
                        let binding = site.binding;
                        presets.push(Hir::silent(HirKind::Define { binding, value }, span));
                    }
                }
                Entry::Registered => {}
            }
        }
        Ok(presets)
    }

    /// Analyze a `def` or `var` form: each name it binds is a new binding, or
    /// the binding its body pre-bound for the first `def` of the name. A lambda
    /// in the value reads the new binding. Each merge frame open in the scope
    /// records it.
    pub(in crate::hir::analyze) fn analyze_def_site(
        &mut self,
        items: &[Syntax],
        form_is_def: bool,
        span: Span,
    ) -> Result<Hir, String> {
        let form = if form_is_def { "def" } else { "var" };
        let target = &items[1];
        let pattern = Self::is_destructure_pattern(target);
        if !pattern && target.as_symbol().is_none() {
            return Err(format!("{}: {} name must be a symbol", span, form));
        }
        let owned = self.owned_def(DefSite::of(items)).cloned();
        let mut sites = Vec::new();
        for (name, scopes, at_mutable) in Self::def_names(target) {
            let key = NameKey::new(self.symbols.intern(name), scopes);
            let first = self
                .scopes
                .last()
                .is_some_and(|s| !s.declared.contains_key(&key));
            let mutable = self.def_is_mutable(form_is_def, pattern, at_mutable);
            self.declare(&key, name, mutable, &target.span)?;
            self.track(&key, name, scopes);
            let prebound = owned
                .as_ref()
                .and_then(|o| o.binding_of(name).map(|b| (b, o.owner)));
            let (binding, entry, assigned) = match prebound {
                Some((binding, owner)) => (
                    binding,
                    Entry::Owned(owner),
                    self.hoist(binding, name, scopes, owner, span),
                ),
                None => {
                    let binding = self.arena.alloc(key.sym, BindingScope::Local);
                    // A first `def` no body owns binds before its value.
                    let entry = if first && !pattern {
                        let in_function = self.scopes.iter().any(|s| s.is_function);
                        self.arena.get_mut(binding).is_prebound = !in_function;
                        self.register_binding(name, scopes, binding);
                        Entry::Registered
                    } else {
                        self.register_defining(name, scopes, binding);
                        Entry::Defining
                    };
                    (binding, entry, false)
                }
            };
            self.arena.get_mut(binding).is_immutable = !mutable;
            sites.push(DefSiteName {
                key,
                name: name.to_string(),
                scopes: scopes.to_vec(),
                binding,
                entry,
                assigned,
            });
        }

        let value = if pattern || sites[0].assigned {
            let value = self.analyze_expr(&items[2])?;
            if !pattern {
                self.last_squelch_signal = None;
            }
            value
        } else {
            self.analyze_definition(sites[0].binding, &items[2])?
        };
        let analyzed = if pattern {
            self.pre_bindings = sites.iter().map(|s| (s.name.clone(), s.binding)).collect();
            let analyzed =
                self.analyze_destructure_pattern(target, BindingScope::Local, form_is_def, &span);
            self.pre_bindings.clear();
            Some(analyzed?)
        } else {
            None
        };
        let mut exprs = self.settle_value_reads(&mut sites, span)?;

        let node = match analyzed {
            Some(analyzed) => self.destructure(analyzed, value, true, span),
            None => {
                let target = sites[0].binding;
                let signal = value.signal;
                let value = Box::new(value);
                let kind = if sites[0].assigned {
                    HirKind::Assign { target, value }
                } else {
                    HirKind::Define {
                        binding: target,
                        value,
                    }
                };
                Hir::new(kind, span, signal)
            }
        };
        let signal = node.signal;
        exprs.push(node);
        for site in sites {
            exprs.extend(self.assign_merges(&site.key, site.binding, span));
            match site.entry {
                Entry::Owned(_) => {
                    self.spend(&site.name, site.binding);
                    self.register_binding(&site.name, &site.scopes, site.binding);
                }
                Entry::Defining => self.reveal(&site.name, site.binding),
                Entry::Registered => {}
            }
        }
        if exprs.len() == 1 {
            return Ok(exprs.remove(0));
        }
        Ok(Hir::new(HirKind::Begin(exprs), span, signal))
    }
}
