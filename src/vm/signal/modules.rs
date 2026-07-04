use super::*;

impl VM {
    /// Force-dispatch a closure on a specific tier.
    ///
    /// `arg` is a list `(tier closure arg1 arg2 ...)`. Routes to the
    /// matching `invoke_closure_*` method on `VM`.
    pub(super) fn dispatch_compile_run_on(
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
                        format!("compile/run-on: malformed args list ({})", e),
                    ),
                )
            }
        };

        if parts.len() < 2 {
            return (
                SIG_ERROR,
                ctx.error(
                    "arity-error",
                    "compile/run-on: expected (tier closure & args), got fewer than 2 parts",
                ),
            );
        }

        let tier_kw = match parts[0].as_keyword_name() {
            Some(k) => k,
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "compile/run-on: tier must be a keyword, got {}",
                            parts[0].type_name()
                        ),
                    ),
                )
            }
        };

        let closure_val = parts[1];
        let closure = match closure_val.as_closure() {
            Some(c) => c.clone(),
            None => {
                return (
                    SIG_ERROR,
                    ctx.error(
                        "type-error",
                        format!(
                            "compile/run-on: target must be a closure, got {}",
                            closure_val.type_name()
                        ),
                    ),
                )
            }
        };

        let call_args: Vec<Value> = parts[2..].to_vec();

        // Set the active tier on the VM for the duration of the forced-tier call
        // (saved and restored, so nested `compile/run-on` and the surrounding
        // "bytecode" default both hold). A `&mut VM`-borrowing guard would conflict
        // with the `invoke_closure_*` call below, so this is an explicit
        // save/replace/restore on the field.
        match tier_kw.as_str() {
            "bytecode" => {
                let prev = std::mem::replace(&mut self.active_tier, "bytecode");
                let r = self.invoke_closure_bytecode(closure_val, &closure, &call_args);
                self.active_tier = prev;
                r
            }
            "jit" => {
                let prev = std::mem::replace(&mut self.active_tier, "jit");
                let r = self.invoke_closure_jit(closure_val, &closure, &call_args);
                self.active_tier = prev;
                r
            }
            #[cfg(feature = "wasm")]
            "wasm" => {
                let prev = std::mem::replace(&mut self.active_tier, "wasm");
                let r = self.invoke_closure_wasm(closure_val, &closure, &call_args);
                self.active_tier = prev;
                r
            }
            #[cfg(not(feature = "wasm"))]
            "wasm" => crate::rich_error!(
                ctx,
                "tier-rejected",
                "compile/run-on :wasm requires --features wasm",
                tier = Value::keyword("wasm"),
                reason = Value::keyword("feature-disabled"),
            ),
            #[cfg(feature = "mlir")]
            "mlir-cpu" => {
                let prev = std::mem::replace(&mut self.active_tier, "mlir-cpu");
                let r = self.invoke_closure_mlir_cpu(closure_val, &closure, &call_args);
                self.active_tier = prev;
                r
            }
            #[cfg(not(feature = "mlir"))]
            "mlir-cpu" => crate::rich_error!(
                ctx,
                "tier-rejected",
                "compile/run-on :mlir-cpu requires --features mlir",
                tier = Value::keyword("mlir-cpu"),
                reason = Value::keyword("feature-disabled"),
            ),
            other => crate::rich_error!(
                ctx,
                "tier-rejected",
                format!("compile/run-on: unknown tier :{}", other),
                tier = parts[0],
                reason = Value::keyword("unknown-tier"),
            ),
        }
    }
    /// Handle `(compile/barrier-module source name)` — compile the file in the
    /// per-form fault-barrier test mode and execute its setup module, returning
    /// the `[index thunk]` accumulator. See `compile_barrier_module` and
    /// docs/test-runner.md § Mechanism.
    ///
    /// Mirrors `eval`'s re-entrant execution: the module bytecode runs on this
    /// VM via `execute_bytecode_saving_stack` (preserving the caller's stack), so
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
    /// accumulator. Mirrors `dispatch_barrier_module`; the legacy multi-form path.
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

        self.execute_test_setup(ctx, result)
    }
    /// `(compile/whole-module-syntax forms name)` — like `dispatch_whole_module`,
    /// but compiles from a list of already-parsed syntax values (shipped from
    /// another VM) instead of a source string. The worker that receives the
    /// shipped syntax runs this against ITS OWN symbol table + stdlib, so a file's
    /// runtime `import`s and the worker's `ev/run` scheduler agree on the dynamic
    /// scheduler parameters (the dual-stdlib `*spawn*` identity fix).
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
                Some(s) => syntaxes.push(s.clone()),
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

        self.execute_test_setup(ctx, result)
    }
    /// Execute a compiled test-setup module on this VM (preserving the caller's
    /// stack) and return its `[index thunk]` accumulator. Shared by the
    /// source-text (`dispatch_test_module`) and syntax (`dispatch_whole_module_syntax`)
    /// compile paths.
    pub(super) fn execute_test_setup(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        result: crate::pipeline::CompileResult,
    ) -> (SignalBits, Value) {
        let bc = result.bytecode;
        let mut code = crate::value::Code::new(
            Rc::new(bc.instructions),
            Rc::new(bc.constants),
            Rc::new(bc.location_map),
            Rc::new(bc.child_protos),
        );
        // Carry the module body's builder-idiom merge metadata (mint-or-reuse;
        // docs/impl/region-model.md § Merging). Empty unless a merge fired.
        code.merged_slots = bc.merged_slots;
        let empty_env = Rc::new(vec![]);
        // Drive the module body, including any nested fiber/resume SIG_SWITCH
        // trampoline, to completion — see VM::run_thunk_to_completion.
        let bits = self.run_thunk_to_completion(&code, &empty_env);

        match bits {
            SIG_OK => {
                let (_, v) = self.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
                (SIG_OK, v)
            }
            SIG_ERROR => {
                let (_, e) = self.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
                (SIG_ERROR, e)
            }
            other => (
                SIG_ERROR,
                ctx.error(
                    "barrier-error",
                    format!("compile/whole-module-syntax: unexpected signal {}", other),
                ),
            ),
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
        let mut map = BTreeMap::new();
        for (kind, text) in dumps {
            map.insert(
                TableKey::from_value(&Value::keyword(&kind)).unwrap(),
                ctx.string(text),
            );
        }
        (SIG_OK, ctx.struct_from(map))
    }
}
