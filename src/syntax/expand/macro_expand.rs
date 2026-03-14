//! Macro call expansion via VM evaluation
//!
//! On first invocation, the macro body `(fn (params...) template)` is compiled
//! and stored in `MacroDef.cached_transformer`. Subsequent invocations skip the
//! full analyze/lower/emit pipeline and call the cached closure directly via
//! `VM::call_closure`, passing arguments as `Value`s.
//!
//! Scope preservation: atom arguments (nil, bool, int, float, string, keyword)
//! are passed as their direct `Value` equivalents — they don't participate in
//! binding resolution and wrapping them as syntax objects would change their
//! runtime semantics (e.g., `false` wrapped in a syntax object becomes truthy).
//! Symbols and compound forms are wrapped as `Value::syntax(arg)` to preserve
//! scope sets through the closure call. `from_value()` unwraps syntax objects
//! back to `Syntax`, preserving scopes. `add_scope_recursive` then stamps the
//! intro scope on the result.
//!
//! Arena management: two phases with distinct guard scopes. Phase 1 (closure
//! compilation) has no guard — closures are allocated on HEAP_ARENA via `alloc()`
//! and a guard would free them. The one-time compilation cost stays in the arena.
//! Phase 2 (closure call + result conversion) has its own guard that frees the
//! transient result values after converting to owned Syntax. This keeps the
//! per-invocation arena cost constant after the first call.
//!
//! Known limitations:
//! - Macros cannot return improper lists (e.g. `(cons 1 2)`). The
//!   `from_value()` conversion requires proper lists.

use super::{Expander, MacroDef, SyntaxKind, MAX_MACRO_EXPANSION_DEPTH};
use crate::symbol::SymbolTable;
use crate::syntax::Syntax;
use crate::value::Value;
use crate::vm::VM;

/// Convert a macro argument Syntax node directly to a Value for passing
/// to a cached closure call. Mirrors `wrap_macro_arg` but produces a
/// `Value` instead of a `Syntax` node.
///
/// Atoms become their direct Value equivalents. Symbols and compounds
/// become `Value::syntax(arg)` to preserve scope sets through the
/// closure call.
fn wrap_macro_arg_value(arg: &Syntax) -> Value {
    match &arg.kind {
        SyntaxKind::Nil => Value::NIL,
        SyntaxKind::Bool(b) => {
            if *b {
                Value::TRUE
            } else {
                Value::FALSE
            }
        }
        SyntaxKind::Int(n) => Value::int(*n),
        SyntaxKind::Float(f) => Value::float(*f),
        SyntaxKind::String(s) => Value::string(s.clone()),
        SyntaxKind::Keyword(k) => Value::keyword(k),
        _ => Value::syntax(arg.clone()),
    }
}

impl Expander {
    pub(super) fn expand_macro_call(
        &mut self,
        macro_def: &MacroDef,
        args: &[Syntax],
        call_site: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        // Check arity
        if macro_def.rest_param.is_some() {
            if args.len() < macro_def.params.len() {
                return Err(format!(
                    "Macro '{}' expects at least {} arguments, got {}",
                    macro_def.name,
                    macro_def.params.len(),
                    args.len()
                ));
            }
        } else if args.len() != macro_def.params.len() {
            return Err(format!(
                "Macro '{}' expects {} arguments, got {}",
                macro_def.name,
                macro_def.params.len(),
                args.len()
            ));
        }

        // Recursion guard
        self.expansion_depth += 1;
        if self.expansion_depth > MAX_MACRO_EXPANSION_DEPTH {
            self.expansion_depth -= 1;
            return Err(format!(
                "macro expansion depth exceeded {} (possible infinite expansion)",
                MAX_MACRO_EXPANSION_DEPTH
            ));
        }

        let result = self.expand_macro_call_inner(macro_def, args, call_site, symbols, vm);
        self.expansion_depth -= 1;
        result
    }

    fn expand_macro_call_inner(
        &mut self,
        macro_def: &MacroDef,
        args: &[Syntax],
        call_site: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        let span = call_site.span.clone();

        // --- Phase 1: Get or compile the transformer closure (no arena guard) ---
        //
        // Cache miss: compile `(fn (p1 p2 & rest) template)` via eval_syntax.
        // Cache hit: clone the cached Value (cheap — Value is Copy, Rc inside).
        //
        // No ArenaGuard here: the closure is allocated into the root FiberHeap
        // and must persist until stored in the transformer cache. A guard would
        // release it before caching.
        // The closure compilation cost (one-time per pipeline call) is left in
        // the arena; subsequent calls skip this phase entirely.
        let transformer: Value = {
            let cached = *macro_def.cached_transformer.borrow();
            if let Some(v) = cached {
                v
            } else {
                // Build the fn parameter list, including rest param if present.
                let mut param_items: Vec<Syntax> = macro_def
                    .params
                    .iter()
                    .map(|p| Syntax::new(SyntaxKind::Symbol(p.clone()), span.clone()))
                    .collect();
                if let Some(ref rest_name) = macro_def.rest_param {
                    param_items.push(Syntax::new(
                        SyntaxKind::Symbol("&".to_string()),
                        span.clone(),
                    ));
                    param_items.push(Syntax::new(
                        SyntaxKind::Symbol(rest_name.clone()),
                        span.clone(),
                    ));
                }
                let params_list = Syntax::new(SyntaxKind::List(param_items), span.clone());

                // Build `(fn (params...) template)`.
                let fn_expr = Syntax::new(
                    SyntaxKind::List(vec![
                        Syntax::new(SyntaxKind::Symbol("fn".to_string()), span.clone()),
                        params_list,
                        macro_def.template.clone(),
                    ]),
                    span.clone(),
                );

                // Compile and execute to obtain the closure Value.
                let closure_val = crate::pipeline::eval_syntax(fn_expr, self, symbols, vm)?;

                // Store in this MacroDef instance's cache.
                *macro_def.cached_transformer.borrow_mut() = Some(closure_val);

                // Write back to the original entry in the macro map so that
                // subsequent expansions within this pipeline call also benefit.
                // (The CompilationCache's Expander won't see this update, which
                // is acceptable — the cache warms per pipeline call.)
                if let Some(original) = self.macros.get_mut(&macro_def.name) {
                    *original.cached_transformer.borrow_mut() = Some(closure_val);
                }

                closure_val
            }
        };

        // --- Phase 2: Call the closure and convert result (with arena guard) ---
        //
        // The arena guard here covers only the transient allocations from calling
        // the closure and converting the Value result to Syntax. The closure itself
        // (`transformer`) was allocated in Phase 1 outside this guard's scope, so
        // it survives the release. This keeps per-invocation arena cost constant.
        let result_syntax = {
            let _arena_guard = crate::value::heap::ArenaGuard::new();

            // --- Wrap arguments as Values ---
            let closure = transformer.as_closure().ok_or_else(|| {
                format!("Macro '{}': transformer is not a closure", macro_def.name)
            })?;

            // Collect fixed param arg values.
            let mut arg_values: Vec<Value> = args[..macro_def.params.len()]
                .iter()
                .map(wrap_macro_arg_value)
                .collect();

            // Collect rest args — the closure's arity is AtLeast(n) with a rest param,
            // and VM's populate_env with VarargKind::List collects remaining args into
            // an Elle list automatically. Pass them as individual Values.
            if macro_def.rest_param.is_some() {
                for arg in &args[macro_def.params.len()..] {
                    arg_values.push(wrap_macro_arg_value(arg));
                }
            }

            // --- Call the cached closure ---
            let result_value = vm.call_closure(closure, &arg_values)?;

            // --- Convert result back to Syntax ---
            // Must happen before _arena_guard drops and frees result_value.
            Syntax::from_value(&result_value, symbols, span.clone())?
            // _arena_guard drops here, releasing transient allocations.
        };

        // Add intro scope for hygiene.
        let intro_scope = self.fresh_scope();
        let hygienized = self.add_scope_recursive(result_syntax, intro_scope);

        // Continue expanding the result.
        self.expand(hygienized, symbols, vm)
    }
}
