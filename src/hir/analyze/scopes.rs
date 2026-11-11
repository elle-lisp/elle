// audited: 2026-11-09
//! Lexical scopes and name resolution over them.
//!
//! Binding, looking up, and the capture a lookup across a function boundary
//! records.
//!
//! docs/bindings.md
//! docs/macros.md

use super::*;

/// Duplicate-definition guard for an explicit `letrec`. Duplicates are
/// judged by *binding identity* — the
/// interned name plus the hygiene scope set the binding is created with —
/// never by spelling, so a macro-introduced name and a user-written one
/// coexist (docs/bindings.md).
#[derive(Default)]
pub(super) struct DuplicateGuard {
    seen: HashMap<(SymbolId, Vec<ScopeId>), Span>,
}

impl DuplicateGuard {
    /// Record one definition; `Err` on a previously-seen identity. The
    /// scope set is sorted into the key so stamp order cannot defeat
    /// identity. The message format matches the letrec error.
    pub(super) fn check(
        &mut self,
        sym: SymbolId,
        name: &str,
        scopes: &[ScopeId],
        span: &Span,
    ) -> Result<(), String> {
        let mut key_scopes = scopes.to_vec();
        key_scopes.sort_unstable_by_key(|s| s.0);
        if let Some(prev) = self.seen.insert((sym, key_scopes), *span) {
            return Err(format!(
                "{}: duplicate binding '{}' (previously defined at {})",
                span, name, prev
            ));
        }
        Ok(())
    }
}

impl<'a> Analyzer<'a> {
    pub(super) fn push_scope(&mut self, is_function: bool) {
        let start_index = if is_function {
            0
        } else {
            self.scopes.last().map(|s| s.next_local).unwrap_or(0)
        };
        self.scopes.push(Scope::with_start_index(
            is_function,
            start_index,
            self.current_form_intros.clone(),
        ));
    }
    /// Push a definition-environment frame: the global frame or a file's
    /// top-level letrec frame. Bindings here are what a macro template's
    /// free variables resolve to, so `lookup()` exempts these frames from
    /// the referential-transparency rule.
    pub(super) fn push_definition_scope(&mut self) {
        self.push_scope(false);
        if let Some(scope) = self.scopes.last_mut() {
            scope.definition_env = true;
        }
    }
    /// Pop the innermost scope. A merged binding still waiting there is never
    /// read: nothing in the scope runs after it.
    pub(super) fn pop_scope(&mut self) -> Option<Scope> {
        self.discard_pending(self.scopes.len().saturating_sub(1));
        self.scopes.pop()
    }
    pub(super) fn bind(&mut self, name: &str, scopes: &[ScopeId], scope: BindingScope) -> Binding {
        let sym = self.symbols.intern(name);
        let binding = self.arena.alloc(sym, scope);
        self.push_entry(
            sym,
            ScopedBinding::new(scopes, binding, EntryState::Visible),
        );
        binding
    }
    /// Register an already-created binding in the current scope without
    /// creating a new one, visible from here on: a file's redefinition, and a
    /// `def` once its value is analyzed.
    pub(super) fn register_binding(&mut self, name: &str, scopes: &[ScopeId], binding: Binding) {
        let entry = ScopedBinding::new(scopes, binding, EntryState::Visible);
        self.push_entry(SymbolId::of(name), entry);
    }
    /// Register a body's first `def` of a name before the analysis reaches
    /// it (`EntryState::Forward`).
    pub(super) fn register_forward(&mut self, name: &str, scopes: &[ScopeId], binding: Binding) {
        let entry = ScopedBinding::new(scopes, binding, EntryState::Forward);
        self.push_entry(SymbolId::of(name), entry);
    }
    /// Turn the current scope's `Forward` entry for `binding` into a `Spent`
    /// one: its `def` has been analyzed.
    pub(super) fn spend(&mut self, name: &str, binding: Binding) {
        let Some(scope) = self.scopes.last_mut() else {
            return;
        };
        if let Some(candidates) = scope.bindings.get_mut(&SymbolId::of(name)) {
            for entry in candidates.iter_mut() {
                if entry.binding == binding && entry.state == EntryState::Forward {
                    entry.state = EntryState::Spent;
                }
            }
        }
    }
    /// Add `entry` for the name `sym` to the current scope.
    fn push_entry(&mut self, sym: SymbolId, entry: ScopedBinding) {
        let local = matches!(self.arena.get(entry.binding).scope, BindingScope::Local);
        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame.bindings.entry(sym).or_default().push(entry);
            if local {
                scope_frame.next_local += 1;
            }
        }
    }
    /// Bind a symbol by its already-interned SymbolId.
    ///
    /// Used by `bind_primitives`, which holds ids from `PrimitiveMeta`. The id
    /// is the scope key, so this needs no spelling — and must not want one:
    /// this analyzer's memo may never have learned the primitives' spellings.
    /// A primitive has empty scopes, so it is visible everywhere.
    pub(super) fn bind_by_sym(&mut self, sym: SymbolId, scope: BindingScope) -> Binding {
        let binding = self.arena.alloc(sym, scope);
        self.push_entry(sym, ScopedBinding::new(&[], binding, EntryState::Visible));
        binding
    }
    pub(super) fn lookup(&mut self, name: &str, ref_scopes: &[ScopeId]) -> Option<Binding> {
        self.resolve(name, ref_scopes, false)
    }

    /// `lookup`, past every binding the innermost scope holds: the binding a
    /// body sees from outside.
    pub(super) fn lookup_outside(&mut self, name: &str, ref_scopes: &[ScopeId]) -> Option<Binding> {
        self.resolve(name, ref_scopes, true)
    }

    fn resolve(
        &mut self,
        name: &str,
        ref_scopes: &[ScopeId],
        skip_innermost: bool,
    ) -> Option<Binding> {
        let mut found_in_scope = None;
        let mut crossed_function_boundary = false;
        let mut skipped_pending = false;
        let mut forward = false;
        let innermost = self.scopes.len().saturating_sub(1);

        // Walk scopes from innermost to outermost
        for (depth, scope) in self.scopes.iter().enumerate().rev() {
            let skipped = skip_innermost && depth == innermost;
            if let Some(candidates) = scope.bindings.get(&SymbolId::of(name)).filter(|_| !skipped) {
                // Find the best candidate: binding's scopes must be a subset of
                // the reference's scopes, and the largest scope set wins.
                // When multiple candidates share the largest scope-set size,
                // max_by_key returns the last one (the most recently bound),
                // which gives correct file-level redefinition semantics.
                //
                // Referential transparency (docs/macros.md): outside a
                // definition-environment frame,
                // a binding is visible to a reference only if every INTRO
                // scope the reference carries is on the binding or in the
                // frame's expansion provenance. A template-origin reference
                // (intro scope present) therefore skips use-site local
                // shadows (intro absent from binder and frame) and resolves
                // at top level, while a template binder (same intro scope)
                // and a datum->syntax binder inside a template-origin form
                // (frame provenance carries the intro) still bind their own
                // expansion's references, and argument-origin / datum->syntax
                // references (no intro scope) still see call-site bindings.
                let best = candidates
                    .iter()
                    .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                    .filter(|c| {
                        scope.definition_env
                            || ref_scopes.iter().all(|s| {
                                !s.is_intro()
                                    || c.scopes.contains(s)
                                    || scope.intro_provenance.contains(s)
                            })
                    })
                    .filter(|c| match c.state {
                        EntryState::Visible => true,
                        // Its `def` is analyzed, and no binding it registered
                        // is visible here: the read is on a path that skips
                        // the `def`.
                        EntryState::Spent => {
                            skipped_pending = true;
                            false
                        }
                        // A direct read in a function body reads past the
                        // body's first `def` of the name (docs/bindings.md).
                        EntryState::Forward => {
                            let direct = !scope.definition_env
                                && self.arena.get(c.binding).prebind_fn_depth == self.fn_depth;
                            skipped_pending |= direct;
                            !direct
                        }
                    })
                    .max_by_key(|c| c.scopes.len());
                if let Some(winner) = best {
                    forward = winner.state == EntryState::Forward;
                    found_in_scope = Some((depth, winner.binding, crossed_function_boundary));
                    break;
                }
            }
            if scope.is_function {
                crossed_function_boundary = true;
            }
        }

        self.defs.skipped_pending |= skipped_pending;
        if let Some((_found_depth, binding, needs_capture)) = found_in_scope {
            self.defs.uses.note_read(binding);
            if forward {
                self.read_forward(binding);
            }
            if needs_capture {
                // Primitives are immutable locals with known constant values.
                // They don't need capturing — the lowerer emits LoadConst
                // for them directly from immutable_values.
                if self.primitive_values.contains_key(&binding) {
                    return Some(binding);
                }

                // A capture is a SELF-edge — the closure captures its own enclosing
                // letrec/def binding, classified `CaptureKind::Recursive` (carrying the
                // SCC binding) — only when it resolves to the binding whose initializer
                // is being analyzed AND the reference sits *directly* in that
                // initializer's lambda: one function level below the letrec/def
                // (`current_init_binding_depth + 1`). A reference from a further-nested
                // lambda is deeper, so it is that inner lambda's sibling capture of the
                // enclosing binding, never this binding's own self-edge — materializing
                // it must yield the enclosing binding, not the inner lambda. A mutable
                // binding may be reassigned, so its reference reads the binding. Every
                // other binding is a sibling/foreign `Local`. The lowerer reads this fact
                // to resolve a self-reference to the executing closure
                // (lir/lower/expr/var.rs).
                let capture_kind = if self.current_init_binding == Some(binding)
                    && self.fn_depth == self.current_init_binding_depth + 1
                    && self.arena.get(binding).is_immutable
                {
                    CaptureKind::Recursive { binding }
                } else {
                    CaptureKind::Local
                };

                // A self-edge does NOT mark the binding captured. A binding captured
                // ONLY by self-references therefore has `needs_capture() == false`
                // (`hir/arena.rs`) — no forward cell — and its self-reference resolves
                // to the executing closure (`LoadSelf` / a self-call), never a cell
                // load, making a self-recursive local `loop` RC-identical to a
                // top-level recursive `defn`. A SIBLING/foreign capture DOES mark, so a
                // binding a *different* closure captures keeps its forward cell (which
                // the closure-cycle merge collapses for mutual recursion). The split is
                // decided here, once: self-edges don't mark, sibling edges do.
                if !matches!(capture_kind, CaptureKind::Recursive { .. }) {
                    self.arena.get_mut(binding).mark_captured();
                }

                // Add to current captures if not already present
                if !self.current_captures.iter().any(|c| c.binding == binding) {
                    self.current_captures.push(CaptureInfo {
                        binding,
                        kind: capture_kind,
                    });
                }
            }
            return Some(binding);
        }

        // If not found in scopes, check if it's in parent captures (for nested lambdas)
        if !self.parent_captures.is_empty() {
            for (capture_index, parent_cap) in self.parent_captures.iter().enumerate() {
                if self.arena.get(parent_cap.binding).name.0 == self.symbols.intern(name).0 {
                    // Found in parent captures - create a transitive capture
                    let binding = parent_cap.binding;

                    // Mark as captured
                    self.arena.get_mut(binding).mark_captured();

                    // Create a Capture kind that references the parent's capture index
                    let capture_kind = CaptureKind::Capture {
                        index: capture_index as u16,
                    };

                    // Add to current captures if not already present
                    if !self.current_captures.iter().any(|c| c.binding == binding) {
                        self.current_captures.push(CaptureInfo {
                            binding,
                            kind: capture_kind,
                        });
                    }

                    return Some(binding);
                }
            }
        }

        None
    }
    /// Use-before-init check for an explicit `letrec` (docs/bindings.md
    /// "Use before initialization is an error"). A direct value read of a
    /// `letrec` binding whose initializer has not yet been analyzed — at
    /// the SAME function depth it was bound at — is a compile error.
    /// A read at a deeper `fn_depth` is inside a lambda: the legal
    /// deferred forward reference (the call runs after every initializer).
    pub(super) fn check_initialized(
        &self,
        binding: Binding,
        name: &str,
        span: &Span,
    ) -> Result<(), String> {
        let inner = self.arena.get(binding);
        if inner.init_pending && inner.prebind_fn_depth == self.fn_depth {
            return Err(format!(
                "{}: '{}' referenced before its initialization",
                span, name
            ));
        }
        Ok(())
    }
    pub(super) fn current_local_count(&self) -> u16 {
        self.scopes.last().map(|s| s.next_local).unwrap_or(0)
    }
    /// Check if a binding is accessible in the current scope stack without crossing a function boundary
    pub(super) fn is_binding_in_current_scope(&self, binding: Binding) -> bool {
        // Walk scopes from innermost to outermost, stopping at function boundaries
        for scope in self.scopes.iter().rev() {
            if scope
                .bindings
                .values()
                .flat_map(|v| v.iter())
                .any(|sb| sb.binding == binding)
            {
                return true;
            }
            if scope.is_function {
                // Stop at function boundary - anything beyond requires capturing
                break;
            }
        }
        false
    }
    /// Look up a binding in only the current (innermost) scope, not walking up the scope chain
    pub(super) fn lookup_in_current_scope(
        &self,
        name: &str,
        ref_scopes: &[ScopeId],
    ) -> Option<Binding> {
        self.scopes.last().and_then(|scope| {
            scope
                .bindings
                .get(&SymbolId::of(name))
                .and_then(|candidates| {
                    candidates
                        .iter()
                        .filter(|c| c.state == EntryState::Visible)
                        .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                        .max_by_key(|c| c.scopes.len())
                        .map(|c| c.binding)
                })
        })
    }
}
