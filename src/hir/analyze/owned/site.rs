// audited: 2026-11-09
//! The analysis of one `def` or `var` site: a new binding of each name, or the one its body pre-bound.
//!
//! docs/bindings.md

use super::super::*;
use super::{DefSite, NameKey};
use crate::syntax::Syntax;

/// One name a `def` site binds, while the site is analyzed.
struct DefSiteName {
    key: NameKey,
    name: String,
    scopes: Vec<ScopeId>,
    binding: Binding,
    /// Its body sets it first, so the `def` assigns it.
    hoisted: bool,
    /// It is visible already, so its value can refer to it.
    registered: bool,
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
        let inner = self.arena.get_mut(binding);
        inner.is_mutated = true;
        inner.is_prebound = true;
        true
    }

    /// Analyze a `def` or `var` form: each name it binds is a new binding,
    /// visible after its value, or the binding its body pre-bound for the
    /// first `def` of the name. Each merge frame open in the scope records it.
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
            let (binding, hoisted, registered) = match prebound {
                Some((binding, owner)) => (
                    binding,
                    self.hoist(binding, name, scopes, owner, span),
                    false,
                ),
                None => {
                    let binding = self.arena.alloc(key.sym, BindingScope::Local);
                    // A first `def` no body owns binds before its value, so its
                    // lambda can call itself.
                    let registered = first && !pattern;
                    if registered {
                        let in_function = self.scopes.iter().any(|s| s.is_function);
                        self.arena.get_mut(binding).is_prebound = !in_function;
                        self.register_binding(name, scopes, binding);
                    }
                    (binding, false, registered)
                }
            };
            self.arena.get_mut(binding).is_immutable = !mutable;
            sites.push(DefSiteName {
                key,
                name: name.to_string(),
                scopes: scopes.to_vec(),
                binding,
                hoisted,
                registered,
            });
        }

        let node = if pattern {
            let value = self.analyze_expr(&items[2])?;
            self.pre_bindings = sites.iter().map(|s| (s.name.clone(), s.binding)).collect();
            let analyzed =
                self.analyze_destructure_pattern(target, BindingScope::Local, form_is_def, &span);
            self.pre_bindings.clear();
            self.destructure(analyzed?, value, true, span)
        } else if sites[0].hoisted {
            let binding = sites[0].binding;
            let value = self.analyze_expr(&items[2])?;
            self.last_import_projection = None;
            self.last_squelch_signal = None;
            let signal = value.signal;
            let value = Box::new(value);
            Hir::new(
                HirKind::Assign {
                    target: binding,
                    value,
                },
                span,
                signal,
            )
        } else {
            let binding = sites[0].binding;
            let value = Box::new(self.analyze_definition(binding, &items[2])?);
            let signal = value.signal;
            Hir::new(HirKind::Define { binding, value }, span, signal)
        };

        let mut exprs = vec![node];
        for site in sites {
            exprs.extend(self.assign_merges(&site.key, site.binding, span));
            if owned.is_some() {
                self.spend(&site.name, site.binding);
            }
            if !site.registered {
                self.register_binding(&site.name, &site.scopes, site.binding);
            }
        }
        if exprs.len() == 1 {
            return Ok(exprs.remove(0));
        }
        let signal = exprs[0].signal;
        Ok(Hir::new(HirKind::Begin(exprs), span, signal))
    }
}
