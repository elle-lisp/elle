// audited: 2026-11-21
//! The `compile/*` queries that compile a test file or dump its stages, and
//! run the setup module of a test file on this VM.
//!
//! docs/test-runner.md
//! docs/impl/region/park.md

use super::*;

/// The error a query answers with, ready to return.
pub(super) type Refusal = (SignalBits, Value);

impl VM {
    /// Handle `(compile/barrier-module source name)` — compile the file in the
    /// per-form fault-barrier test mode and execute its setup module, returning
    /// the `[index thunk]` accumulator. See `compile_barrier_module` and
    /// docs/test-runner.md.
    ///
    /// Mirrors `eval`'s re-entrant execution: the module bytecode runs on this
    /// VM via `run_thunk_to_completion`, which preserves the caller's stack. So
    /// `def`/`var` setup forms run, and the thunk closures are created on the
    /// bytecode tier, where `MakeClosure` is legal. A compile failure, or a
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
        // so a top-level `(signal :kw)` declaration here spends none of the 32 user
        // bits; the worker's execution compile registers what the file declares.
        // The returned thunk is not run from this compile, so dropping the
        // registration is safe.
        let snapshot = crate::signals::registry::snapshot_registry();
        let result =
            self.dispatch_test_module(ctx, arg, "compile/whole-module", |src, syms, cctx, name| {
                crate::pipeline::compile_whole_module(src, syms, cctx, name)
            });
        crate::signals::registry::restore_registry(snapshot);
        result
    }
    /// Shared body for the test compilation queries (`compile/barrier-module`,
    /// `compile/whole-module`). It parses `(source name)`, fetches the context
    /// symbol table and compiles via `compile_fn`. Then it executes the setup
    /// module on this VM, preserving the caller's stack, to yield the
    /// `[index thunk]` accumulator. `prim` names the calling primitive for
    /// error messages.
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
        ) -> Result<crate::value::CodeUnit, String>,
    ) -> (SignalBits, Value) {
        let (source, name) = match Self::source_and_name(ctx, arg, prim) {
            Ok(pair) => pair,
            Err(refusal) => return refusal,
        };
        let compiled = self.with_compiler(ctx, prim, |symbols, cctx| {
            compile_fn(&source, symbols, cctx, &name)
        });
        let result = match compiled {
            Ok(Ok(r)) => r,
            Ok(Err(msg)) => return (SIG_ERROR, ctx.error("compile-error", msg)),
            Err(refusal) => return refusal,
        };

        self.execute_test_setup(ctx, result, prim)
    }
    /// The arguments of `prim`'s query, refused where the list is malformed
    /// or holds fewer than `count`, which `shape` spells for the error.
    pub(super) fn query_args(
        ctx: &crate::primitives::ctx::Alloc,
        arg: Value,
        prim: &str,
        shape: &str,
        count: usize,
    ) -> Result<Vec<Value>, Refusal> {
        let parts = arg.list_to_vec_in(ctx.heap_mut()).map_err(|e| {
            (
                SIG_ERROR,
                ctx.error(
                    "type-error",
                    format!("{}: malformed args list ({})", prim, e),
                ),
            )
        })?;
        if parts.len() < count {
            return Err((
                SIG_ERROR,
                ctx.error("arity-error", format!("{}: expected {}", prim, shape)),
            ));
        }
        Ok(parts)
    }
    /// `value` as a string, refused as a type error that names `what`.
    pub(super) fn string_arg(
        ctx: &crate::primitives::ctx::Alloc,
        value: Value,
        prim: &str,
        what: &str,
    ) -> Result<String, Refusal> {
        value.with_string(|s| s.to_string()).ok_or_else(|| {
            (
                SIG_ERROR,
                ctx.error("type-error", format!("{}: {} must be a string", prim, what)),
            )
        })
    }
    /// The two strings of a `(source name)` query.
    pub(super) fn source_and_name(
        ctx: &crate::primitives::ctx::Alloc,
        arg: Value,
        prim: &str,
    ) -> Result<(String, String), Refusal> {
        let parts = Self::query_args(ctx, arg, prim, "(source name)", 2)?;
        let source = Self::string_arg(ctx, parts[0], prim, "source")?;
        let name = Self::string_arg(ctx, parts[1], prim, "name")?;
        Ok((source, name))
    }
    /// Run `f` with this instance's symbol table and compile context, on
    /// behalf of the calling fiber. Refused where either is unavailable.
    pub(super) fn with_compiler<R>(
        &mut self,
        ctx: &crate::primitives::ctx::Alloc,
        prim: &str,
        f: impl FnOnce(&mut crate::symbol::SymbolTable, &mut crate::pipeline::CompileCtx) -> R,
    ) -> Result<R, Refusal> {
        // The table is reached through this VM, as eval reaches it. A raw
        // deref lets it sit beside the `compile_ctx` borrow below.
        let symbols_ptr = self.symbols_ptr;
        if symbols_ptr.is_null() {
            return Err((
                SIG_ERROR,
                ctx.error(
                    "compile-error",
                    format!("{}: symbol table not available (not set in context)", prim),
                ),
            ));
        }
        let symbols = unsafe { &mut *symbols_ptr };
        let withheld = self.fiber.withheld;
        let Some(cctx) = self.compile_ctx() else {
            return Err((
                SIG_ERROR,
                ctx.error(
                    "compile-error",
                    format!("{}: compile context unavailable", prim),
                ),
            ));
        };
        Ok(cctx.on_behalf_of(withheld, |cctx| f(symbols, cctx)))
    }
    /// `(compile/whole-module-syntax forms name)` — like `dispatch_whole_module`,
    /// but compiles from a list of already-parsed syntax values (shipped from
    /// another VM) instead of a source string. The worker that receives the
    /// shipped syntax runs this against ITS OWN symbol table + stdlib. So a
    /// file's runtime `import`s and the worker's `ev/run` scheduler read the
    /// same dynamic scheduler parameters, such as `*spawn*` and `*loader*`.
    pub(super) fn dispatch_whole_module_syntax(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        arg: Value,
    ) -> (SignalBits, Value) {
        let prim = "compile/whole-module-syntax";
        let named = Self::query_args(ctx, arg, prim, "(forms name)", 2)
            .and_then(|parts| Ok((Self::string_arg(ctx, parts[1], prim, "name")?, parts)));
        let (name, parts) = match named {
            Ok(named) => named,
            Err(refusal) => return refusal,
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

        let compiled = self.with_compiler(ctx, prim, |symbols, cctx| {
            crate::pipeline::compile_whole_module_forms(syntaxes, symbols, cctx, &name)
        });
        let result = match compiled {
            Ok(Ok(r)) => r,
            Ok(Err(msg)) => return (SIG_ERROR, ctx.error("compile-error", msg)),
            Err(refusal) => return refusal,
        };

        self.execute_test_setup(ctx, result, prim)
    }
    /// Execute a compiled test-setup module on this VM (preserving the caller's
    /// stack) and return its `[index thunk]` accumulator. Shared by the
    /// source-text (`dispatch_test_module`) and syntax (`dispatch_whole_module_syntax`)
    /// compile paths. `prim` names the calling primitive for error messages.
    pub(super) fn execute_test_setup(
        &mut self,
        ctx: &mut crate::primitives::ctx::Alloc,
        unit: crate::value::CodeUnit,
        prim: &str,
    ) -> (SignalBits, Value) {
        // The entry carries the module body's builder-idiom merge metadata
        // (mint-or-reuse; docs/impl/region/merging.md) with the rest of its
        // payload. Empty unless a merge fired. The unit runs on this VM's heap
        // and is held until the run ends, because the entry's `Code` takes no
        // reference of its own.
        let unit = unit.on_heap(self.heap());
        let code = unit.entry().code();
        let empty_env = Rc::new(vec![]);
        // Drive the module body, including any nested fiber/resume SIG_SWITCH
        // trampoline, to completion — see VM::run_thunk_to_completion.
        let depth = self.fiber.param_depth();
        let bits = self.run_thunk_to_completion(&code, &empty_env);

        match bits {
            SIG_OK => {
                let (_, v) = self.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
                // The setup module's accumulator left its compiled top level
                // through the return convention, so it carries its return mint.
                // The raising primitives declare `result_minted`, so the invoking
                // `dispatch_native_call` skips the pass-through retain for
                // this answer.
                (SIG_OK, v)
            }
            b if b.intersects(SIG_ERROR) && !b.intersects(crate::value::SIG_HALT) => {
                // The error parked the module's chain, which the setup run
                // cannot hold; the error leaves as the run's own.
                self.discard_hosted_error(b, depth);
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
    /// `--dump` artifacts as a struct `{:kind string}` (docs/test-runner.md).
    /// Mirrors `dispatch_barrier_module`'s symbol-table
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

        let prim = "compile/dumps";
        let (source, name) = match Self::source_and_name(ctx, arg, prim) {
            Ok(pair) => pair,
            Err(refusal) => return refusal,
        };
        let rendered = self.with_compiler(ctx, prim, |symbols, cctx| {
            let dumps = crate::dump::render_all(&source, &name, symbols, cctx);
            // The stage names are build-fixed and in the vocabulary, but the
            // memo is right here and a stage added later would otherwise print
            // as a hash until somebody noticed.
            for kind in dumps.keys() {
                symbols.keyword(kind);
            }
            dumps
        });
        let dumps = match rendered {
            Ok(dumps) => dumps,
            Err(refusal) => return refusal,
        };
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
