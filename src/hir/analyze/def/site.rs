// audited: 2026-11-24
//! One `def` or `var` site: each name it binds is a new binding, visible from the site to the end of its body.
//!
//! docs/bindings.md

use super::super::*;
use super::NameKey;
use crate::hir::pattern::HirPattern;
use crate::syntax::Syntax;

/// What one `def` or `var` site bound, with its analyzed value.
pub(in crate::hir::analyze) enum DefBound {
    /// A symbol target: the binding it names.
    Name(Binding, Hir),
    /// A pattern target: the pattern that binds its leaves from the value.
    Pattern(HirPattern, Hir),
}

impl<'a> Analyzer<'a> {
    /// Analyze a `def` or `var` form. A `def` that does not stand directly in
    /// a body is refused (`placement.rs`). A `def` at the global scope binds a
    /// pre-bound binding, as a `def` of the file does.
    pub(in crate::hir::analyze) fn analyze_def_site(
        &mut self,
        items: &[Syntax],
        form_is_def: bool,
        span: Span,
    ) -> Result<Hir, String> {
        let form = if form_is_def { "def" } else { "var" };
        self.check_placement(form, span)?;
        let in_function = self.scopes.iter().any(|s| s.is_function);
        let global = !in_function && self.scopes.last().is_some_and(|s| s.definition_env);
        Ok(
            match self.bind_def(&items[1], &items[2], form_is_def, global, span)? {
                DefBound::Name(binding, value) => {
                    let signal = value.signal;
                    let value = Box::new(value);
                    Hir::new(HirKind::Define { binding, value }, span, signal)
                }
                DefBound::Pattern(pattern, value) => self.destructure(pattern, value, true, span),
            },
        )
    }

    /// Bind each name `target` names to a new binding in the current scope,
    /// and analyze `value`. A symbol is visible while its value is analyzed,
    /// so a lambda in the value reads it. A direct read there is the error a
    /// pending `letrec` binding gives. A pattern analyzes its value first, and
    /// binds its leaves as `let` does. `prebound` marks each binding pre-bound.
    pub(in crate::hir::analyze) fn bind_def(
        &mut self,
        target: &Syntax,
        value: &Syntax,
        form_is_def: bool,
        prebound: bool,
        span: Span,
    ) -> Result<DefBound, String> {
        let pattern = Self::is_destructure_pattern(target);
        if !pattern && target.as_symbol().is_none() {
            let form = if form_is_def { "def" } else { "var" };
            return Err(format!(
                "{}: {} name must be a symbol or destructure pattern",
                target.span, form
            ));
        }
        let mut fresh: Vec<(&str, &[ScopeId], Binding)> = Vec::new();
        for (name, scopes, at_mutable) in Self::def_names(target) {
            let key = NameKey::new(self.symbols.intern(name), scopes);
            let mutable = self.def_is_mutable(form_is_def, pattern, at_mutable);
            self.declare(&key, name, mutable, &target.span)?;
            // A pattern that names a leaf twice binds it once.
            if fresh.iter().any(|(n, _, _)| *n == name) {
                continue;
            }
            let binding = self.arena.alloc(key.sym, BindingScope::Local);
            let inner = self.arena.get_mut(binding);
            inner.is_immutable = !mutable;
            inner.is_prebound = prebound;
            if !pattern {
                inner.site = Some(target.span);
            }
            fresh.push((name, scopes, binding));
        }

        if !pattern {
            let (name, scopes, binding) = fresh[0];
            let fn_depth = self.fn_depth;
            let inner = self.arena.get_mut(binding);
            inner.init_pending = true;
            inner.prebind_fn_depth = fn_depth;
            self.register_binding(name, scopes, binding);
            let value = self.analyze_definition(binding, value);
            self.arena.get_mut(binding).init_pending = false;
            return Ok(DefBound::Name(binding, value?));
        }

        let value = self.analyze_expr(value)?;
        // A squelch fact the value left names no leaf.
        self.last_squelch_signal = None;
        self.pre_bindings = fresh.iter().map(|(n, _, b)| (n.to_string(), *b)).collect();
        let analyzed =
            self.analyze_destructure_pattern(target, BindingScope::Local, form_is_def, &span);
        self.pre_bindings.clear();
        let analyzed = analyzed?;
        for (name, scopes, binding) in fresh {
            self.register_binding(name, scopes, binding);
        }
        Ok(DefBound::Pattern(analyzed, value))
    }
}
