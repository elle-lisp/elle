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
//! scope sets through the closure call.
//!
//! Hygiene (Flatt 2016, sets of scopes): each expansion mints a fresh intro
//! scope, PRE-STAMPS it on the wrapped arguments, and after the transformer
//! returns, FLIPS it on the whole result (`flip_scope_recursive`): template-
//! origin identifiers — which never saw the scope — gain it, and argument-
//! origin identifiers lose it, recovering their use-site scope sets exactly.
//! A template binder therefore carries the intro scope while inbound
//! identifiers do not, so under the subset resolution rule the binder cannot
//! capture them (`tests/integration/macro_hygiene.rs`). `datum->syntax`
//! (scope_exempt) opts a node out of both operations — the deliberate-capture
//! escape hatch.
//!
//! Arena management: two phases. Phase 1 (closure compilation) is NOT scoped —
//! the cached transformer closure must survive every call, so it is allocated on
//! the root FiberHeap via `alloc()` and reclaimed only at teardown
//! (`release_cached_transformers`). The one-time compilation cost stays resident.
//! Phase 2 (closure call + result conversion) is a CLOSED ALLOCATION SCOPE
//! (docs/impl/region/rules.md § "Macro expansion — a closed allocation scope"):
//! a per-call mint log records every region the transformer mints, and after the
//! result is deep-copied to owned Syntax the scope is reclaimed by balancing each
//! survivor's unexplained references. This keeps the per-invocation region cost
//! constant — without it every expansion leaked its construction scratch (the
//! dominant teardown residue, the `Pair` class).
//!
//! Known limitations:
//! - Macros cannot return improper lists (e.g. `(pair 1 2)`). The
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
///
/// These are **ordinary mortal allocations** born in the per-expansion transient
/// `region` the sole caller mints (docs/impl/region/ctx.md — the region is named
/// explicitly as an argument). That region is part of the expansion's closed allocation scope
/// (docs/impl/region/rules.md), so the wrapped args are reclaimed with the rest
/// of the transformer's scratch once the result is deep-copied to owned Syntax —
/// they do not leak per expansion. Only the heap cases (`String`, compound
/// `_ => syntax`) take the region; the atom cases are immediates with no region.
fn wrap_macro_arg_value(
    heap: &mut crate::value::fiberheap::FiberHeap,
    arg: &Syntax,
    region: crate::hir::region::RuntimeRegion,
) -> Value {
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
        SyntaxKind::String(s) => crate::value::build::string(heap, s.clone(), region),
        SyntaxKind::Keyword(k) => Value::keyword(k),
        _ => crate::value::build::syntax(heap, arg.clone(), region),
    }
}

impl Expander {
    /// Compile (once) and cache a macro's transformer closure `(fn (params)
    /// template)`, returning the closure `Value`.
    ///
    /// Cache hit: return the stored closure (cheap — `Value` is `Copy`). Cache
    /// miss: compile via `eval_syntax` and store into BOTH `macro_def`'s cell
    /// (the within-call clone) and the authoritative `self.macros[name]` entry.
    ///
    /// The compiled closure lives in a solver-assigned region from the nested
    /// compilation; that region is NEVER freed by dropping the `Value` (`Copy`,
    /// no `Drop`). Whoever owns the surviving cache entry owns the region —
    /// which is why prelude/core transformers are pre-compiled ONCE into the
    /// persistent compilation-cache master (`precompile_transformers`) and
    /// released at teardown (`release_cached_transformers`), instead of being
    /// re-compiled into each per-compile `Expander` clone and orphaned when the
    /// clone drops (the corpus-OOM per-compile leak).
    /// NativeFn allocations inside the body (quasiquote `Value::syntax` wrappers,
    /// string literals) survive as that closure's bytecode constants, so they
    /// must share its region, not a transient one.
    pub(crate) fn ensure_transformer(
        &mut self,
        macro_def: &MacroDef,
        span: &crate::syntax::Span,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Value, String> {
        // Prefer the authoritative map entry's cache (it may have been compiled
        // by a nested expansion since `macro_def` was cloned) over the clone's.
        if let Some(v) = self
            .macros
            .get(&macro_def.name)
            .and_then(|d| *d.cached_transformer.borrow())
        {
            *macro_def.cached_transformer.borrow_mut() = Some(v);
            return Ok(v);
        }
        if let Some(v) = *macro_def.cached_transformer.borrow() {
            return Ok(v);
        }

        // Build the fn parameter list: required, &opt optional, & rest.
        let mut param_items: Vec<Syntax> = macro_def
            .params
            .iter()
            .map(|p| Syntax::new(SyntaxKind::Symbol(p.clone()), span.clone()))
            .collect();
        if !macro_def.optional_params.is_empty() {
            param_items.push(Syntax::new(
                SyntaxKind::Symbol("&opt".to_string()),
                span.clone(),
            ));
            for p in &macro_def.optional_params {
                param_items.push(Syntax::new(SyntaxKind::Symbol(p.clone()), span.clone()));
            }
        }
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

        let closure_val = crate::pipeline::eval_syntax(fn_expr, self, symbols, vm)?;

        // Store in the within-call clone AND write back to the authoritative
        // entry so subsequent expansions (this pipeline call, or — for the
        // master — every future compile) reuse it.
        *macro_def.cached_transformer.borrow_mut() = Some(closure_val);
        if let Some(original) = self.macros.get_mut(&macro_def.name) {
            *original.cached_transformer.borrow_mut() = Some(closure_val);
        }
        Ok(closure_val)
    }

    pub(super) fn expand_macro_call(
        &mut self,
        macro_def: &MacroDef,
        args: &[Syntax],
        call_site: &Syntax,
        symbols: &mut SymbolTable,
        vm: &mut VM,
    ) -> Result<Syntax, String> {
        // Check arity: required params must be present, optional and rest are flexible
        let min_args = macro_def.params.len();
        let max_args = min_args + macro_def.optional_params.len();
        if macro_def.rest_param.is_some() {
            if args.len() < min_args {
                return Err(format!(
                    "Macro '{}' expects at least {} arguments, got {}",
                    macro_def.name,
                    min_args,
                    args.len()
                ));
            }
        } else if args.len() < min_args || args.len() > max_args {
            if min_args == max_args {
                return Err(format!(
                    "Macro '{}' expects {} arguments, got {}",
                    macro_def.name,
                    min_args,
                    args.len()
                ));
            } else {
                return Err(format!(
                    "Macro '{}' expects {}-{} arguments, got {}",
                    macro_def.name,
                    min_args,
                    max_args,
                    args.len()
                ));
            }
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

        // --- Phase 1: Get or compile the transformer closure ---
        let transformer: Value = self.ensure_transformer(macro_def, &span, symbols, vm)?;

        // --- Phase 2: Call the closure and convert result ---
        //
        // The per-expansion transient `region_id` is the explicit allocation
        // region for the Rust-side argument wrapping below (docs/impl/region/ctx.md
        // — named explicitly as an argument). The transformer's own body allocates elsewhere:
        // through its ctx (native calls mint fresh result regions) and its
        // activation region map (data instructions). Those regions, plus
        // `region_id`, are all captured by the mint scope opened below and
        // reclaimed together once `Syntax::from_value` has deep-copied the
        // result to owned Syntax (which builds owned Syntax, not Values).
        if transformer.as_closure().is_none() {
            return Err(format!(
                "Macro '{}': transformer is not a closure",
                macro_def.name
            ));
        }

        let opt_start = macro_def.params.len();
        let opt_end = opt_start + macro_def.optional_params.len();
        let opt_provided = args.len().min(opt_end);

        // Hygiene (sets of scopes): mint this expansion's intro scope and
        // PRE-STAMP it on every argument before the transformer runs. After
        // the call, flip_scope_recursive removes it from argument-origin
        // nodes and adds it to template-origin nodes — distinguishing the
        // two without tracking provenance through the transformer.
        let intro_scope = self.fresh_scope();

        // Macro expansion is a CLOSED ALLOCATION SCOPE (docs/impl/region/rules.md
        // § "Macro expansion — a closed allocation scope"): the transformer's
        // entire `Value` output is deep-copied to owned `Syntax` below, so every
        // region it mints — the arg-wrap region `region_id`, the constructed
        // output tree, and the scratch its constructors discard internally — is
        // dead afterward. Open a mint log around the whole call so the reclaim
        // pass can balance each survivor's unexplained references by RC. The
        // reclaim covers every region — root, interior, and orphan scratch — and
        // subsumes the explicit `region_id` free.
        crate::value::arena::begin_macro_scope(unsafe { &mut *vm.heap_ptr });
        // Mint this expansion's transient arg region explicitly: the Rust-side
        // argument wrapping below allocates into it. It is part of the mint scope
        // and reclaimed with the rest.
        let region_id = vm.heap().new_runtime_region();
        // The expansion's heap, reached through the VM's raw `heap_ptr` (a `Copy`
        // pointer that holds no borrow), so the `stamp` closure stays `Fn + Copy`
        // — it captures the pointer, not a `&mut VM` — and the `.map(stamp)` reuse
        // pattern below keeps compiling while every wrapped arg is born on this
        // instance's own heap.
        let heap_ptr = vm.heap_ptr;
        let result_syntax = (|| {
            // Wrap arguments as Values born in this expansion's transient region
            // on this instance's heap.
            let region = region_id;
            let stamp = |arg: &Syntax| -> Value {
                wrap_macro_arg_value(
                    unsafe { &mut *heap_ptr },
                    &self.add_scope_recursive(arg.clone(), intro_scope),
                    region,
                )
            };
            let mut arg_values: Vec<Value> =
                args[..macro_def.params.len()].iter().map(stamp).collect();
            for arg in &args[opt_start..opt_provided] {
                arg_values.push(stamp(arg));
            }
            if macro_def.rest_param.is_some() {
                for arg in &args[opt_end..] {
                    arg_values.push(stamp(arg));
                }
            }

            crate::value::fiberheap::freelog::set_context(format!(
                "macro '{}' at {}",
                macro_def.name,
                span.clone()
            ));
            // Publish this expansion's intro scope while the transformer
            // runs: datum->syntax strips it from copied context scopes so
            // its results carry the context's TRUE use-site scope set
            // (nested expansions save/restore — strictly synchronous).
            let prev_intro = crate::syntax::set_current_macro_intro(Some(intro_scope));
            let call_result = vm.call_closure(transformer, &arg_values);
            crate::syntax::set_current_macro_intro(prev_intro);
            let result_value = call_result?;
            // Deep-copy the result to owned Syntax while the transformer's
            // scratch is still live; the scope reclaim below then frees ALL of
            // it (the result tree's root and interior, the arg region, and any
            // constructor-discarded orphan), balancing each survivor's
            // unexplained references by RC. The result tree carried a Rule-5
            // `ReturnValue` escape on its root and one on every tail-flowing
            // node whose decref the solver suppressed; the reclaim balances them
            // uniformly, so no per-result decref is needed here.
            Syntax::from_value(&result_value, symbols, span.clone())
        })();
        // Close the mint scope: reclaim every region the transformer minted that
        // is not held alive by a live edge (and is not a process-lifetime root).
        // Runs on both Ok and Err — on error there is no expansion to keep, so
        // all scratch is reclaimed too.
        crate::value::arena::reclaim_macro_scope(unsafe { &mut *vm.heap_ptr });
        let result_syntax = result_syntax?;

        // Hygiene flip: template-origin identifiers (never saw the intro
        // scope) gain it; argument-origin identifiers (pre-stamped above)
        // lose it, restoring their use-site scope sets exactly.
        let hygienized = self.flip_scope_recursive(result_syntax, intro_scope);

        // Continue expanding the result.
        self.expand(hygienized, symbols, vm)
    }
}
