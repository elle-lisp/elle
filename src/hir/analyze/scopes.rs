use super::*;

/// Duplicate-definition guard for `letrec*` contexts (explicit `letrec`,
/// fn-body `begin`). Duplicates are judged by *binding identity* — the
/// interned name plus the hygiene scope set the binding is created with —
/// never by spelling, so a macro-introduced name and a user-written one
/// coexist (docs/bindings.md "Duplicates are judged by binding identity").
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
        if let Some(prev) = self.seen.insert((sym, key_scopes), span.clone()) {
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
    pub(super) fn pop_scope(&mut self) -> Option<Scope> {
        self.scopes.pop()
    }
    pub(super) fn bind(&mut self, name: &str, scopes: &[ScopeId], scope: BindingScope) -> Binding {
        let sym = self.symbols.intern(name);
        let binding = self.arena.alloc(sym, scope);

        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame
                .bindings
                .entry(SymbolId::of(name))
                .or_default()
                .push(ScopedBinding {
                    scopes: scopes.to_vec(),
                    binding,
                });
            if matches!(scope, BindingScope::Local) {
                scope_frame.next_local += 1;
            }
        }

        binding
    }
    /// Register an already-created binding in the current scope without
    /// creating a new one. Used by `analyze_file_letrec` Pass 2 to add
    /// deferred duplicate-name bindings at the correct sequential point.
    pub(super) fn register_binding(&mut self, name: &str, scopes: &[ScopeId], binding: Binding) {
        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame
                .bindings
                .entry(SymbolId::of(name))
                .or_default()
                .push(ScopedBinding {
                    scopes: scopes.to_vec(),
                    binding,
                });
            if matches!(self.arena.get(binding).scope, BindingScope::Local) {
                scope_frame.next_local += 1;
            }
        }
    }
    /// Bind a symbol by its already-interned SymbolId.
    ///
    /// Used by `bind_primitives`, which holds ids from `PrimitiveMeta`. The id
    /// is the scope key, so this needs no spelling — and must not want one:
    /// those ids were minted against the compile context's table, which is not
    /// this analyzer's.
    pub(super) fn bind_by_sym(&mut self, sym: SymbolId, scope: BindingScope) -> Binding {
        let binding = self.arena.alloc(sym, scope);

        if let Some(scope_frame) = self.scopes.last_mut() {
            scope_frame
                .bindings
                .entry(sym)
                .or_default()
                .push(ScopedBinding {
                    scopes: Vec::new(), // primitives have empty scopes (visible everywhere)
                    binding,
                });
            if matches!(scope, BindingScope::Local) {
                scope_frame.next_local += 1;
            }
        }

        binding
    }
    pub(super) fn lookup(&mut self, name: &str, ref_scopes: &[ScopeId]) -> Option<Binding> {
        let mut found_in_scope = None;
        let mut crossed_function_boundary = false;

        // Walk scopes from innermost to outermost
        for (depth, scope) in self.scopes.iter().enumerate().rev() {
            if let Some(candidates) = scope.bindings.get(&SymbolId::of(name)) {
                // Find the best candidate: binding's scopes must be a subset of
                // the reference's scopes, and the largest scope set wins.
                // When multiple candidates share the largest scope-set size,
                // max_by_key returns the last one (the most recently bound),
                // which gives correct file-level redefinition semantics.
                //
                // Referential transparency (docs/macros.md § The Hygiene
                // Problem, point 2): outside a definition-environment frame,
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
                    .max_by_key(|c| c.scopes.len());
                if let Some(winner) = best {
                    found_in_scope = Some((depth, winner.binding, crossed_function_boundary));
                    break;
                }
            }
            if scope.is_function {
                crossed_function_boundary = true;
            }
        }

        if let Some((_found_depth, binding, needs_capture)) = found_in_scope {
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
                // it must yield the enclosing binding, not the inner lambda. Every other
                // binding is a sibling/foreign `Local`. The lowerer reads this fact to
                // resolve a self-reference to the executing closure (lir/lower/lambda.rs).
                let capture_kind = if self.current_init_binding == Some(binding)
                    && self.fn_depth == self.current_init_binding_depth + 1
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
    /// Use-before-init check for `letrec*` contexts (docs/bindings.md
    /// "Use before initialization is an error"). A direct value read of a
    /// prebound binding whose initializer has not yet been analyzed — at
    /// the SAME function depth it was prebound at — is a compile error.
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
                        .filter(|c| is_scope_subset(&c.scopes, ref_scopes))
                        .max_by_key(|c| c.scopes.len())
                        .map(|c| c.binding)
                })
        })
    }
}
