//! Hygienic macro expansion

mod collections;
mod compiletime;
mod define;
mod introspection;
mod macro_expand;
mod quasiquote;
mod syntaxcase;
#[cfg(test)]
mod tests;

use super::{ScopeId, Span, Syntax, SyntaxKind};
use crate::primitives::def::PrimitiveMeta;
use crate::symbol::SymbolTable;
use crate::vm::VM;
use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

/// Maximum macro expansion depth before erroring (prevents infinite expansion)
const MAX_MACRO_EXPANSION_DEPTH: usize = 200;

/// Macro definition stored as Syntax
#[derive(Debug, Clone)]
pub struct MacroDef {
    pub name: String,
    pub params: Vec<String>,
    /// Optional parameters (after `&opt`, before any `&` rest).
    pub optional_params: Vec<String>,
    pub rest_param: Option<String>,
    pub template: Syntax,
    #[allow(dead_code)] // set during construction; read access planned for hygiene
    pub(crate) definition_scope: ScopeId,
    /// Cached compiled transformer closure (compiled from `(fn (params...)
    /// template)`), populated LAZILY on first expansion so its quoted-literal
    /// hygiene captures the real expansion context (eager compilation at a
    /// different point mis-scopes template literals — e.g. `each`'s `'in`).
    ///
    /// `Rc<RefCell<…>>` so the cell is SHARED across `Expander`/`MacroDef`
    /// clones: every per-compile clone of the compilation-cache master aliases
    /// the master's cell, so the first compile that expands the macro fills it
    /// ONCE and every later compile reuses it. Without the share, each clone
    /// re-compiled the transformer into a fresh region and orphaned it on clone
    /// drop (`Value` is `Copy`, no decref) — the corpus-OOM per-compile leak. The
    /// owning region is released at teardown by `release_cached_transformers`.
    pub(crate) cached_transformer: std::rc::Rc<RefCell<Option<crate::value::Value>>>,
}

/// Hygienic macro expander
pub struct Expander {
    macros: HashMap<String, MacroDef>,
    /// Compile-time values defined by `begin-for-syntax` blocks.
    /// Always starts empty — the manual `Clone` impl resets it so that
    /// compile-time defs never leak between pipeline calls via the cache.
    pub(crate) compile_time_env: HashMap<String, crate::value::Value>,
    /// Pre-prelude exports from core.lisp. Persists across clones so
    /// that macro bodies compiled via `eval_syntax` can reference
    /// core functions like `last` and `butlast`.
    pub(crate) core_env: HashMap<String, crate::value::Value>,
    /// Primitive (+ stdlib export) metadata used to compile macro-transformer
    /// bodies via `eval_syntax`. `Rc` so that the per-pipeline-call clone is a
    /// pointer bump rather than a deep copy of the maps. The owning instance's
    /// `CompileCtx` sets this to `primitives` at construction and to
    /// `primitives + stdlib exports` once `init_stdlib` runs; REPL value
    /// bindings are deliberately NOT included (macro bodies never resolve them).
    /// Because it rides on the `Expander`, `eval_syntax` reaches it without a
    /// separate `CompileCtx` borrow — which would alias the macro VM mid-expand.
    eval_meta: Rc<PrimitiveMeta>,
    next_scope_id: u32,
    expansion_depth: usize,
}

impl Clone for Expander {
    fn clone(&self) -> Self {
        Expander {
            macros: self.macros.clone(),
            compile_time_env: HashMap::new(), // always fresh — never inherit compile-time defs
            core_env: self.core_env.clone(),  // persists — needed by macro bodies
            eval_meta: Rc::clone(&self.eval_meta), // shared pointer; immutable after stdlib load
            next_scope_id: self.next_scope_id,
            expansion_depth: self.expansion_depth,
        }
    }
}

/// Scope operation applied by `map_scope_recursive`. Both operations honor
/// `scope_exempt`: a datum->syntax node keeps exactly the scopes its context
/// gave it. The context's scopes at transformer time include the expansion's
/// pre-stamped intro scope, which datum->syntax itself strips (see
/// `prim_datum_to_syntax`), so an exempt node already carries its TRUE
/// use-site scope set and must dodge the flip.
#[derive(Clone, Copy)]
enum ScopeOp {
    Add,
    Flip,
}

impl Expander {
    pub fn new() -> Self {
        Expander {
            macros: HashMap::new(),
            compile_time_env: HashMap::new(),
            core_env: HashMap::new(),
            eval_meta: Rc::new(PrimitiveMeta::default()),
            next_scope_id: 1, // 0 is reserved for top-level
            expansion_depth: 0,
        }
    }

    /// The primitive(+stdlib) metadata for compiling macro-transformer bodies
    /// (`eval_syntax`). See the `eval_meta` field.
    pub fn eval_meta(&self) -> &PrimitiveMeta {
        &self.eval_meta
    }

    /// Replace the macro-body compile metadata. The owning `CompileCtx` calls
    /// this with `primitives` at construction and `primitives + stdlib exports`
    /// after `init_stdlib`.
    pub fn set_eval_meta(&mut self, meta: PrimitiveMeta) {
        self.eval_meta = Rc::new(meta);
    }

    /// Register a macro definition
    pub fn define_macro(&mut self, def: MacroDef) {
        self.macros.insert(def.name.clone(), def);
    }

    /// Check if any macros are registered (used to detect if prelude is loaded)
    pub fn has_macros(&self) -> bool {
        !self.macros.is_empty()
    }

    /// Return the macro definitions. Used by the REPL to persist
    /// macros defined during expansion back to the compilation cache.
    pub fn macros(&self) -> &HashMap<String, MacroDef> {
        &self.macros
    }

    /// Merge macro definitions from another Expander. Existing macros
    /// with the same name are overwritten. Used to persist REPL-defined
    /// macros back to the compilation cache.
    pub fn merge_macros(&mut self, other: &HashMap<String, MacroDef>) {
        for (name, def) in other {
            self.macros.insert(name.clone(), def.clone());
        }
    }

    /// Load the standard prelude macros.
    ///
    /// Parses and expands `prelude.lisp`, which registers macro
    /// definitions in this Expander. Must be called after the VM
    /// has primitives registered but before user code expansion.
    pub fn load_prelude(&mut self, symbols: &mut SymbolTable, vm: &mut VM) -> Result<(), String> {
        const PRELUDE: &str = include_str!("../../prelude.lisp");
        let syntaxes = crate::reader::read_syntax_all(PRELUDE, "<internal>")?;
        // Use ScopeId(0) — the reserved primitive scope — so that
        // prelude symbols match primitive bindings (which are also
        // bound with ScopeId(0)). This is critical for macro hygiene:
        // template symbols in quasiquotes carry ScopeId(0), allowing
        // them to resolve to primitives even when the call site has
        // shadowing bindings with the same name.
        let prelude_scope = ScopeId(0);
        for syntax in syntaxes {
            let scoped = self.add_scope_recursive(syntax, prelude_scope);
            self.expand(scoped, symbols, vm)?;
        }
        Ok(())
    }

    /// Release the region reference each cached transformer holds, dropping the
    /// cache entries to `None`. The transformer closures live in regions that a
    /// plain `Drop` of the `Value` (it is `Copy`) would never decref, so without
    /// this they survive teardown as residue. Called from `CompileCtx::release`
    /// on the instance's master expander at teardown, when it is the last holder
    /// of these shared transformer cells (per-compile clones, which share the
    /// SAME `Rc` cell, are long gone).
    pub fn release_cached_transformers(&mut self, heap: &mut crate::value::fiberheap::FiberHeap) {
        for def in self.macros.values() {
            if let Some(v) = def.cached_transformer.borrow_mut().take() {
                let r = crate::value::arena::region_of(heap, v);
                crate::value::arena::decref_region(heap, r);
            }
        }
    }

    /// Generate a fresh scope ID
    pub(crate) fn fresh_scope(&mut self) -> ScopeId {
        let id = ScopeId(self.next_scope_id);
        self.next_scope_id += 1;
        id
    }

    /// Create a symbol syntax node
    fn make_symbol(&self, name: &str, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(name.to_string()), span)
    }

    /// Create a list syntax node
    fn make_list(&self, items: Vec<Syntax>, span: Span) -> Syntax {
        Syntax::new(SyntaxKind::List(items), span)
    }

    /// Stamp a fresh file scope onto a syntax tree. This distinguishes
    /// user bindings from primitives (which have empty scopes), enabling
    /// macro hygiene: template symbols from the prelude carry
    /// `ScopeId(0)` and won't match user bindings that carry the file
    /// scope instead.
    pub fn stamp_file_scope(&mut self, syntax: Syntax) -> Syntax {
        let scope = self.fresh_scope();
        self.add_scope_recursive(syntax, scope)
    }

    /// Public wrapper for adding a scope to a syntax tree.
    pub(crate) fn stamp_scope(&self, syntax: Syntax, scope: ScopeId) -> Syntax {
        self.add_scope_recursive(syntax, scope)
    }

    /// Expand all macros in a syntax tree
    pub fn expand(
        &mut self,
        syntax: Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        // Point the transformer-running VM at the table THIS expansion uses, so a
        // transformer's `gensym`/`syntax->datum`/`read` (via `ctx.vm().symbols()`)
        // interns into the same table the expander threads. Idempotent across
        // recursion; the table is reborrowed per transformer call, never touched
        // simultaneously (docs/impl/region-ctx.md § "Symbols").
        vm.set_symbols(symbols as *mut SymbolTable);
        match &syntax.kind {
            SyntaxKind::Symbol(_) => Ok(syntax),
            SyntaxKind::List(items) if !items.is_empty() => {
                // Check if first element is a symbol
                if let Some(name) = items[0].as_symbol() {
                    // Handle defmacro specially - register and return nil
                    if name == "defmacro" || name == "define-macro" {
                        return self.handle_defmacro(items, &syntax.span);
                    }

                    // Handle macro introspection
                    if name == "macro?" {
                        return self.handle_macro_predicate(items, &syntax.span);
                    }
                    if name == "expand-macro" {
                        return self.handle_expand_macro(items, &syntax.span, symbols, vm);
                    }

                    if name == "begin-for-syntax" {
                        return self.handle_begin_for_syntax(items, &syntax.span, symbols, vm);
                    }

                    if name == "syntax-case" {
                        return self.handle_syntax_case(items, &syntax.span, symbols, vm);
                    }

                    // Check if it's a macro call
                    if let Some(macro_def) = self.macros.get(name).cloned() {
                        return self.expand_macro_call(
                            &macro_def,
                            &items[1..],
                            &syntax,
                            symbols,
                            vm,
                        );
                    }
                }
                // Not a macro call - expand children recursively
                self.expand_seq(
                    items,
                    syntax.span,
                    syntax.scopes,
                    symbols,
                    vm,
                    SyntaxKind::List,
                )
            }
            SyntaxKind::Array(items) => self.expand_seq(
                items,
                syntax.span,
                syntax.scopes,
                symbols,
                vm,
                SyntaxKind::Array,
            ),
            SyntaxKind::ArrayMut(items) => self.expand_seq(
                items,
                syntax.span,
                syntax.scopes,
                symbols,
                vm,
                SyntaxKind::ArrayMut,
            ),
            SyntaxKind::Struct(items) => self.expand_seq(
                items,
                syntax.span,
                syntax.scopes,
                symbols,
                vm,
                SyntaxKind::Struct,
            ),
            SyntaxKind::StructMut(items) => self.expand_seq(
                items,
                syntax.span,
                syntax.scopes,
                symbols,
                vm,
                SyntaxKind::StructMut,
            ),
            SyntaxKind::Set(items) => self.expand_seq(
                items,
                syntax.span,
                syntax.scopes,
                symbols,
                vm,
                SyntaxKind::Set,
            ),
            SyntaxKind::SetMut(items) => self.expand_seq(
                items,
                syntax.span,
                syntax.scopes,
                symbols,
                vm,
                SyntaxKind::SetMut,
            ),
            SyntaxKind::Quote(_) => {
                // Don't expand inside quote
                Ok(syntax)
            }
            SyntaxKind::Quasiquote(inner) => {
                // Convert quasiquote to code that builds the structure
                self.quasiquote_to_code(inner, 1, &syntax.span, symbols, vm)
            }
            SyntaxKind::Splice(inner) => {
                let expanded = self.expand((**inner).clone(), symbols, vm)?;
                Ok(Syntax::with_scopes(
                    SyntaxKind::Splice(Box::new(expanded)),
                    syntax.span,
                    syntax.scopes,
                ))
            }
            SyntaxKind::StringMut(s) => {
                // @"..." desugars to (thaw "...") at expansion time.
                // The thaw symbol carries ScopeId(0) (primitive scope).
                let thaw_sym = Syntax::with_scopes(
                    SyntaxKind::Symbol("thaw".into()),
                    syntax.span.clone(),
                    vec![ScopeId(0)],
                );
                let str_lit = Syntax::with_scopes(
                    SyntaxKind::String(s.clone()),
                    syntax.span.clone(),
                    syntax.scopes.clone(),
                );
                Ok(Syntax::with_scopes(
                    SyntaxKind::List(vec![thaw_sym, str_lit]),
                    syntax.span,
                    syntax.scopes,
                ))
            }
            _ => Ok(syntax),
        }
    }

    fn add_scope_recursive(&self, syntax: Syntax, scope: ScopeId) -> Syntax {
        self.map_scope_recursive(syntax, scope, ScopeOp::Add)
    }

    /// Flip `scope` on every non-exempt node (see `Syntax::flip_scope`):
    /// the post-expansion hygiene operation. Template-origin identifiers
    /// gain the intro scope; argument-origin identifiers (pre-stamped at
    /// wrap time) lose it again. datum->syntax results are exempt — they
    /// already carry their context's true scopes (the intro scope is
    /// stripped at copy time by `prim_datum_to_syntax`).
    pub(super) fn flip_scope_recursive(&self, syntax: Syntax, scope: ScopeId) -> Syntax {
        self.map_scope_recursive(syntax, scope, ScopeOp::Flip)
    }

    fn map_scope_recursive(&self, mut syntax: Syntax, scope: ScopeId, op: ScopeOp) -> Syntax {
        // datum->syntax nodes keep their exact scopes — neither ordinary
        // stamping nor the hygiene flip touches them.
        if syntax.scope_exempt {
            return syntax;
        }

        match op {
            ScopeOp::Add => syntax.add_scope(scope),
            ScopeOp::Flip => syntax.flip_scope(scope),
        }

        // Recurse into children
        syntax.kind = match syntax.kind {
            SyntaxKind::List(items) => SyntaxKind::List(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::Array(items) => SyntaxKind::Array(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::ArrayMut(items) => SyntaxKind::ArrayMut(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::Struct(items) => SyntaxKind::Struct(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::StructMut(items) => SyntaxKind::StructMut(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::Set(items) => SyntaxKind::Set(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::SetMut(items) => SyntaxKind::SetMut(
                items
                    .into_iter()
                    .map(|item| self.map_scope_recursive(item, scope, op))
                    .collect(),
            ),
            SyntaxKind::Quote(inner) => {
                // Don't add scope inside quote - it's literal data
                SyntaxKind::Quote(inner)
            }
            SyntaxKind::Quasiquote(inner) => {
                SyntaxKind::Quasiquote(Box::new(self.map_scope_recursive(*inner, scope, op)))
            }
            SyntaxKind::Unquote(inner) => {
                SyntaxKind::Unquote(Box::new(self.map_scope_recursive(*inner, scope, op)))
            }
            SyntaxKind::UnquoteSplicing(inner) => {
                SyntaxKind::UnquoteSplicing(Box::new(self.map_scope_recursive(*inner, scope, op)))
            }
            SyntaxKind::Splice(inner) => {
                SyntaxKind::Splice(Box::new(self.map_scope_recursive(*inner, scope, op)))
            }
            // Don't recurse into syntax literals — the inner Value::syntax
            // already carries its correct scopes from the original context.
            SyntaxKind::SyntaxLiteral(_) => syntax.kind,
            other => other,
        };

        syntax
    }
}

impl Default for Expander {
    fn default() -> Self {
        Self::new()
    }
}
