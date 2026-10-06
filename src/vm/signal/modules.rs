// audited: 2026-10-06
//! The `compile/*` queries that compile a test file or dump its stages, and
//! run the setup module of a test file on this VM.
//!
//! docs/test-runner.md
//! docs/impl/region/park.md

use super::*;

impl VM {
    /// Handle `(compile/barrier-module source name)` — compile the file in the
    /// per-form fault-barrier test mode and execute its setup module, returning
    /// the `[index thunk]` accumulator. See `compile_barrier_module` and
    /// docs/test-runner.md § Mechanism.
    ///
    /// Mirrors `eval`'s re-entrant execution: the module bytecode runs on this
    /// VM via `run_thunk_to_completion` (preserving the caller's stack), so
    /// `def`/`var` setup forms run and the thunk closures are created on the
    /// bytecode tier (where `MakeClosure` is legal). A compile failure, or a
    /// def-initializer runtime fault, surfaces as `SIG_ERROR` — the runner's
    /// `protect` turns it into a single file-level failure.
    pub(super) fn dispatch_barrier_module(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        self.dispatch_test_module(
            ctx,
            arg,
            "compile/barrier-module",
            crate::pipeline::compile_barrier_module,
        )
    }
    /// Handle `(compile/whole-module source name)` — compile the file as ONE
    /// whole-file thunk and execute its setup module, returning the `[0 thunk]`
    /// accumulator. Mirrors `dispatch_barrier_module`; the runner compiles a
    /// file of several forms this way.
    pub(super) fn dispatch_whole_module(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        // In the runner this compile is the gating/error DETECTION pass — the file
        // is actually executed from a worker via compile/whole-module-syntax, which
        // shares the process-global signal registry. Keep detection registry-neutral
        // so a top-level `(signal :kw)` declaration here doesn't collide with the
        // worker's execution compile ("already registered"). The returned thunk is
        // not run from this compile, so dropping the registration is safe.
        let snapshot = crate::signals::registry::snapshot_registry();
        let result =
            self.dispatch_test_module(ctx, arg, "compile/whole-module", |src, syms, cctx, name| {
                crate::pipeline::compile_whole_module(src, syms, cctx, name)
            });
        crate::signals::registry::restore_registry(snapshot);
        result
    }
    /// Shared body for the test compilation queries (`compile/barrier-module`,
    /// `compile/whole-module`): parse `(source name)`, fetch the context symbol
    /// table, compile via `compile_fn`, and execute the resulting setup module on
    /// this VM (preserving the caller's stack) to yield the `[index thunk]`
    /// accumulator. `prim` names the calling primitive for error messages.
    pub(super) fn dispatch_test_module(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
        prim: &str,
        compile_fn: impl FnOnce(
            &str,
            &mut crate::symbol::SymbolTable,
            &mut crate::pipeline::CompileCtx,
            &str,
        ) -> Result<crate::pipeline::CompileResult, String>,
    ) -> (SignalBits, Value) {
        let parts = match arg.list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("{}: malformed args list ({})", prim, e),
                    ),
                )
            }
        };
        if parts.len() < 2 {
            return (
                SIG_ERROR,
                ctx.error("arity-error", format!("{}: expected (source name)", prim)),
            );
        }
        let source = match parts[0].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", format!("{}: source must be a string", prim)),
                )
            }
        };
        let name = match parts[1].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", format!("{}: name must be a string", prim)),
                )
            }
        };

        // This instance's symbol table, reached through this VM (same pattern as
        // eval). Raw deref so it sits beside the `compile_ctx` borrow below.
        let symbols_ptr = self.symbols_ptr;
        if symbols_ptr.is_null() {
            return (
                SIG_ERROR,
                ctx.error(
                    "compile-error",
                    format!("{}: symbol table not available (not set in context)", prim),
                ),
            );
        }
        let symbols = unsafe { &mut *symbols_ptr };

        // This instance's compile context, reached through the executing VM.
        let result = {
            let Some(cctx) = self.compile_ctx() else {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "compile-error",
                        format!("{}: compile context unavailable", prim),
                    ),
                );
            };
            match compile_fn(&source, symbols, cctx, &name) {
                Ok(r) => r,
                Err(msg) => return (SIG_ERROR, ctx.error("compile-error", msg)),
            }
        };

        self.execute_test_setup(ctx, result, prim)
    }
    /// `(compile/whole-module-syntax forms name)` — like `dispatch_whole_module`,
    /// but compiles from a list of already-parsed syntax values (shipped from
    /// another VM) instead of a source string. The worker that receives the
    /// shipped syntax runs this against ITS OWN symbol table + stdlib, so a file's
    /// runtime `import`s and the worker's `ev/run` scheduler read the same
    /// dynamic scheduler parameters, such as `*spawn*`.
    pub(super) fn dispatch_whole_module_syntax(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        let parts = match arg.list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("compile/whole-module-syntax: malformed args list ({})", e),
                    ),
                )
            }
        };
        if parts.len() < 2 {
            return (
                SIG_ERROR,
                ctx.error(
                    "arity-error",
                    "compile/whole-module-syntax: expected (forms name)",
                ),
            );
        }
        let name = match parts[1].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        "compile/whole-module-syntax: name must be a string",
                    ),
                )
            }
        };
        // Unwrap the forms list into owned Syntax nodes.
        let form_vals = match parts[0].list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("compile/whole-module-syntax: forms must be a list ({})", e),
                    ),
                )
            }
        };
        let mut syntaxes = Vec::with_capacity(form_vals.len());
        for v in &form_vals {
            match v.as_syntax() {
                Some(s) => syntaxes.push(*s),
                None => {
                    return (
                        SIG_ERROR,
                        ctx.error(
                            "type-error",
                            format!(
                                "compile/whole-module-syntax: every form must be syntax, got {}",
                                v.type_name()
                            ),
                        ),
                    )
                }
            }
        }

        let symbols_ptr = self.symbols_ptr;
        if symbols_ptr.is_null() {
            return (
                SIG_ERROR,
                ctx.error(
                    "compile-error",
                    "compile/whole-module-syntax: symbol table not available (not set in context)",
                ),
            );
        }
        let symbols = unsafe { &mut *symbols_ptr };

        let result = {
            let Some(cctx) = self.compile_ctx() else {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "compile-error",
                        "compile/whole-module-syntax: compile context unavailable",
                    ),
                );
            };
            match crate::pipeline::compile_whole_module_forms(syntaxes, symbols, cctx, &name) {
                Ok(r) => r,
                Err(msg) => return (SIG_ERROR, ctx.error("compile-error", msg)),
            }
        };

        self.execute_test_setup(ctx, result, "compile/whole-module-syntax")
    }
    /// Execute a compiled test-setup module on this VM (preserving the caller's
    /// stack) and return its `[index thunk]` accumulator. Shared by the
    /// source-text (`dispatch_test_module`) and syntax (`dispatch_whole_module_syntax`)
    /// compile paths. `prim` names the calling primitive for error messages.
    pub(super) fn execute_test_setup(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        result: crate::pipeline::CompileResult,
        prim: &str,
    ) -> (SignalBits, Value) {
        // The blueprint carries the module body's builder-idiom merge metadata
        // (mint-or-reuse; docs/impl/region/merging.md § Merging) with the rest of
        // its payload. Empty unless a merge fired.
        let code = crate::value::ClosureTemplate::for_proto(
            self.heap(),
            &Rc::new(result.bytecode.into_proto()),
        )
        .code();
        let empty_env = Rc::new(vec![]);
        // Drive the module body, including any nested fiber/resume SIG_SWITCH
        // trampoline, to completion — see VM::run_thunk_to_completion.
        let depth = self.fiber.param_depth();
        let bits = self.run_thunk_to_completion(&code, &empty_env);

        match bits {
            SIG_OK => {
                let (_, v) = self.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
                // The setup module's accumulator left its compiled top level
                // through the return convention — it carries its return mint,
                // exactly as the module value `import/load-file` runs does. The raising
                // primitives declare `result_minted`, so the invoking
                // `dispatch_native_call` skips the pass-through retain for
                // this answer.
                (SIG_OK, v)
            }
            SIG_ERROR => {
                let (_, e) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                (SIG_ERROR, e)
            }
            other => {
                // The setup run cannot hold a park of the module it ran, so it
                // refuses it and raises at its own call.
                self.refuse_hosted_park(other, depth);
                (
                    SIG_ERROR,
                    ctx.error(
                        "barrier-error",
                        format!("{}: unexpected signal {}", prim, other),
                    ),
                )
            }
        }
    }
    /// `(compile/dumps source name)` — compile a module once and return its
    /// `--dump` artifacts as a struct `{:kind string}` (docs/test-runner.md
    /// § CAS asset capture). Mirrors `dispatch_barrier_module`'s symbol-table
    /// acquisition; the rendering itself lives in `crate::dump` (the single
    /// source of truth shared with `elle --dump`). Each stage is independently
    /// fallible — a kind that doesn't compile is simply absent from the struct,
    /// so a partially-compiling source still returns whatever stages succeeded.
    pub(super) fn dispatch_compile_dumps(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        use crate::value::TableKey;
        use std::collections::BTreeMap;

        let parts = match arg.list_to_vec_in(ctx.heap_mut()) {
            Ok(v) => v,
            Err(e) => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!("compile/dumps: malformed args list ({})", e),
                    ),
                )
            }
        };
        if parts.len() < 2 {
            return (
                SIG_ERROR,
                ctx.error("arity-error", "compile/dumps: expected (source name)"),
            );
        }
        let source = match parts[0].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "compile/dumps: source must be a string"),
                )
            }
        };
        let name = match parts[1].with_string(|s| s.to_string()) {
            Some(s) => s,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error("type-error", "compile/dumps: name must be a string"),
                )
            }
        };

        let symbols_ptr = self.symbols_ptr;
        if symbols_ptr.is_null() {
            return (
                SIG_ERROR,
                ctx.error(
                    "compile-error",
                    "compile/dumps: symbol table not available (not set in context)",
                ),
            );
        }
        let symbols = unsafe { &mut *symbols_ptr };

        let Some(cctx) = self.compile_ctx() else {
            return (
                SIG_ERROR,
                ctx.error(
                    "compile-error",
                    "compile/dumps: compile context unavailable",
                ),
            );
        };
        let dumps = crate::dump::render_all(&source, &name, symbols, cctx);
        let symbols = unsafe { &mut *symbols_ptr };
        let mut map = BTreeMap::new();
        for (kind, text) in dumps {
            // The stage names are build-fixed and in the vocabulary, but the
            // memo is right here and a stage added later would otherwise print
            // as a hash until somebody noticed.
            symbols.keyword(&kind);
            map.insert(
                TableKey::from_value(&Value::keyword(&kind)).unwrap(),
                ctx.string(text),
            );
        }
        (SIG_OK, ctx.struct_from(map))
    }
}
