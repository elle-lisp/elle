// audited: 2026-10-06
//! The three module loaders, for Elle source, a shared library and one form.
//!
//! `import/load-file` runs Elle source, `import/load-plugin` loads a shared
//! library, and `import/load-syntax` runs one form.
//!
//! docs/modules.md
//! docs/impl/region/park.md

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// What a loader hands back: the signal bits and the value, as a primitive does.
type Outcome = (SignalBits, Value);

/// A loader's path argument as an absolute, normalized path.
///
/// A relative path resolves against the working directory, as `slurp` resolves
/// one. The cycle check and the plugin cache key on this spelling, so two
/// spellings of one file are one load. An absolute path also keeps `dlopen`
/// from searching the library path for a bare name.
fn path_arg(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    arg: Value,
    name: &str,
) -> Result<String, Outcome> {
    let Some(path) = arg.with_string(|s| s.to_string()) else {
        return Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!("{name}: expected string, got {}", arg.type_name()),
            ),
        ));
    };
    crate::path::absolute(&path).map_err(|e| {
        crate::rich_error!(
            ctx,
            "io-error",
            format!("{name}: {e}"),
            path = ctx.string(path.as_str()),
        )
    })
}

/// The driving VM, detached from `ctx`'s borrow so the `vm.*` calls and the
/// `ctx.*` allocations — which use the disjoint heap — coexist. `ctx.vm()` is
/// total, since a native always runs under a VM.
fn driving_vm<'v>(ctx: &mut crate::primitives::ctx::NativeCtx<'_>) -> &'v mut crate::vm::VM {
    let vm: *mut crate::vm::VM = ctx.vm();
    unsafe { &mut *vm }
}

/// Trace one load under `--trace=import`.
fn trace_load(vm: &crate::vm::VM, name: &str, path: &str) {
    crate::etrace!(
        vm,
        crate::config::trace_bits::IMPORT,
        "import",
        "{} {}",
        name,
        path
    );
}

/// Mint the caller's owning reference for a plugin value the loader hands along.
///
/// `import/load-plugin` declares
/// [`result_minted`](crate::primitives::def::PrimitiveDef::result_minted), so
/// `dispatch_native_call` takes no pass-through retain for it. The plugin path
/// runs no thunk, so this retain is that reference. It balances the caller's
/// `DecrefValueRegion` exactly as the dispatch retain would have, and leaves the
/// plugin cache's own reference untouched.
fn retain_plugin_result(vm: &mut crate::vm::VM, value: Value) {
    let heap = unsafe { &mut *vm.heap_ptr };
    let region = crate::value::arena::region_of(heap, value);
    crate::value::arena::incref_for_escape(
        heap,
        region,
        crate::value::arena::EscapeSite::NativeCallResult,
    );
}

/// Run `load` under the circular-load mark for `key`.
///
/// The mark brackets the load: everything a load does is the one call below, so
/// every way out of it — a compile error, a read failure, an error the module
/// raised — reaches the single unmark after that call. A load that finds `key`
/// already marked is a cycle, and the error names every file in it.
fn with_load_mark(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    name: &str,
    key: &str,
    load: impl FnOnce(&mut crate::primitives::ctx::NativeCtx<'_>) -> Outcome,
) -> Outcome {
    let vm = driving_vm(ctx);
    if let Some(cycle) = vm.loading_cycle(key) {
        return crate::rich_error!(
            ctx,
            "io-error",
            format!("{name}: circular dependency: {cycle}"),
            path = ctx.string(key),
        );
    }
    vm.mark_module_loading(key.to_string());
    let outcome = load(ctx);
    driving_vm(ctx).unmark_module_loading(key);
    outcome
}

/// Run a compiled module's top level on the driving VM and hand back its value.
///
/// The module's forms run as part of the current fiber's execution, as `eval`'s
/// thunk does, so a top-level `protect` returns the `SIG_SWITCH` trampoline
/// signal, which `run_thunk_to_completion` drains here. A module whose top level
/// suspends is refused: the loader cannot hold a park of the fiber it runs on.
fn run_module(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    name: &str,
    origin: &str,
    compiled: crate::pipeline::CompileResult,
) -> Outcome {
    let vm = driving_vm(ctx);
    let code = crate::value::ClosureTemplate::for_proto(
        vm.heap(),
        &std::rc::Rc::new(compiled.bytecode.into_proto()),
    )
    .code();
    let empty_env = std::rc::Rc::new(vec![]);
    let depth = vm.fiber.param_depth();
    match vm.run_thunk_to_completion(&code, &empty_env) {
        SIG_OK => {
            // The module value left its compiled top level through the return
            // convention, so it already carries the one owed reference the
            // caller's release consumes — the `result_minted` claim.
            let (_, value) = vm.fiber.signal.take().unwrap_or((SIG_OK, Value::NIL));
            (SIG_OK, value)
        }
        SIG_ERROR => {
            let (_, err) = vm.fiber.signal.take().unwrap_or((SIG_ERROR, Value::NIL));
            let msg = vm.format_error_with_location(err);
            crate::rich_error!(
                ctx,
                "eval-error",
                format!("{name}: runtime error in {origin}: {msg}"),
                path = ctx.string(origin),
            )
        }
        bits => {
            vm.refuse_hosted_park(bits, depth);
            crate::rich_error!(
                ctx,
                "eval-error",
                format!("{name}: unexpected signal {bits} in {origin}"),
                path = ctx.string(origin),
            )
        }
    }
}

/// Compile with this instance's symbol table and compile context, reached
/// through the driving VM, on behalf of the calling fiber: the compile's
/// macros run without the capabilities that fiber withholds.
fn compile_with(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    name: &str,
    compile: impl FnOnce(
        &mut crate::symbol::SymbolTable,
        &mut crate::pipeline::CompileCtx,
    ) -> Result<crate::pipeline::CompileResult, String>,
) -> Result<crate::pipeline::CompileResult, String> {
    let withheld = ctx.withheld();
    let vm = driving_vm(ctx);
    let symbols_ptr = vm.symbols_ptr;
    if symbols_ptr.is_null() {
        return Err(format!("{name}: symbol table context not initialized"));
    }
    match vm.compile_ctx() {
        Some(cctx) => {
            cctx.on_behalf_of(withheld, |cctx| compile(unsafe { &mut *symbols_ptr }, cctx))
        }
        None => Err(format!("{name}: compile context unavailable")),
    }
}

/// `(import/load-file path)`: compile the Elle source at `path`, run it, and
/// return its last expression.
pub(crate) fn prim_load_file(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> Outcome {
    const NAME: &str = "import/load-file";
    let path = match path_arg(ctx, args[0], NAME) {
        Ok(p) => p,
        Err(outcome) => return outcome,
    };
    trace_load(driving_vm(ctx), NAME, &path);
    with_load_mark(ctx, NAME, &path.clone(), |ctx| {
        let source = match std::fs::read_to_string(&path) {
            Ok(s) => s,
            Err(e) => {
                return crate::rich_error!(
                    ctx,
                    "io-error",
                    format!("{NAME}: failed to read '{path}': {e}"),
                    path = ctx.string(path.as_str()),
                );
            }
        };
        let compiled = compile_with(ctx, NAME, |symbols, cctx| {
            crate::pipeline::compile_file(&source, symbols, cctx, &path)
        });
        match compiled {
            Ok(compiled) => run_module(ctx, NAME, &path, compiled),
            Err(e) => crate::rich_error!(
                ctx,
                "eval-error",
                format!("{NAME}: compilation error in {path}: {e}"),
                path = ctx.string(path.as_str()),
            ),
        }
    })
}

/// `(import/load-plugin path)`: load the shared library at `path`, run its
/// `elle_plugin_init`, and return the struct of primitives it built. A later
/// call with the same path returns that struct and loads nothing.
pub(crate) fn prim_load_plugin(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> Outcome {
    const NAME: &str = "import/load-plugin";
    let path = match path_arg(ctx, args[0], NAME) {
        Ok(p) => p,
        Err(outcome) => return outcome,
    };
    let vm = driving_vm(ctx);
    if let Some(&cached) = vm.loaded_plugins.get(&path) {
        // `result_minted`: this call ran no thunk to produce the cached value,
        // so it mints the caller's reference here.
        retain_plugin_result(vm, cached);
        return (SIG_OK, cached);
    }
    trace_load(vm, NAME, &path);
    let symbols_ptr = vm.symbols_ptr;
    if symbols_ptr.is_null() {
        return (
            SIG_ERROR,
            ctx.error(
                "internal-error",
                format!("{NAME}: symbol table context not initialized"),
            ),
        );
    }
    match crate::plugin::load_plugin(&path, vm, unsafe { &mut *symbols_ptr }) {
        Ok(value) => {
            vm.loaded_plugins.insert(path, value);
            retain_plugin_result(vm, value);
            (SIG_OK, value)
        }
        Err(e) => crate::rich_error!(
            ctx,
            "io-error",
            format!("{NAME}: {e}"),
            path = ctx.string(path.as_str()),
        ),
    }
}

/// `(import/load-syntax form)`: compile one form as a module, run it, and
/// return its value. A syntax object keeps its own source locations; a plain
/// datum has none.
pub(crate) fn prim_load_syntax(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> Outcome {
    const NAME: &str = "import/load-syntax";
    const ORIGIN: &str = "<syntax>";
    let form = args[0];
    let compiled = compile_with(ctx, NAME, |symbols, cctx| {
        crate::pipeline::compile_value(form, symbols, cctx, ORIGIN)
    });
    match compiled {
        Ok(compiled) => run_module(ctx, NAME, ORIGIN, compiled),
        Err(e) => (
            SIG_ERROR,
            ctx.error("eval-error", format!("{NAME}: compilation error: {e}")),
        ),
    }
}

primitive! {
    "import/load-file" => prim_load_file {
        signal: Signal::fs_errors(),
        arity: Arity::Exact(1),
        doc: "Compile the Elle source file at PATH, run it, and return its last \
              expression. A relative PATH resolves against the working directory, \
              as slurp resolves one. Every call compiles and runs the file again. \
              Code calls import-file or import, so that a relative path follows the \
              file that wrote it.",
        params: &["path"],
        example: "(import/load-file \"tests/modules/test.lisp\")",
        // Opaque, not Mixed: the path is copied out to a Rust String to read the
        // file and never retained, so no argument is stored, while the result —
        // a value the module's own compiled top level returned — lives in
        // neither this call's region nor the path's. The VM re-entry rule:
        // unbounded result, no store (docs/impl/region/effects.md).
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
    "import/load-plugin" => prim_load_plugin {
        signal: Signal::fs_ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Load the shared library at PATH, run its elle_plugin_init, and \
              return the struct of primitives the init builds. A later call with \
              the same path returns that struct and loads nothing. Requires :ffi, \
              because the init is foreign code.",
        params: &["path"],
        example: "(import/load-plugin \"target/release/libelle_regex.so\")",
        // Opaque for the reason import/load-file is: the path is copied out,
        // and the result is a value the plugin's init built and the cache holds.
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
    "import/load-syntax" => prim_load_syntax {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Compile one FORM as a module, run it, and return its value. A \
              syntax object keeps its own source locations.",
        params: &["form"],
        example: "(import/load-syntax '(+ 1 2))",
        // Opaque: the form is copied into the compile's own arena and never
        // retained, and the result is what the module's top level returned.
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
}
