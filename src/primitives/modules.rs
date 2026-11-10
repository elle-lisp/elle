// audited: 2026-11-09
//! The plugin loader: `import/load-plugin`, the one module loader that is a primitive.
//!
//! docs/modules.md
//! docs/impl/region/effects.md
//!
//! Every other loader is Elle code in src/stdlib.lisp. This one runs foreign
//! code, so `:ffi` sits on its declaration.

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::types::Arity;
use crate::value::Value;

/// What a loader hands back: the signal bits and the value, as a primitive does.
type Outcome = (SignalBits, Value);

/// A loader's path argument as an absolute, normalized path.
///
/// A relative path resolves against the working directory, as `file/read`
/// resolves one. The plugin cache keys on this spelling, so two spellings of
/// one file are one load. An absolute path also keeps `dlopen` from searching
/// the library path for a bare name.
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

primitive! {
    "import/load-plugin" => prim_load_plugin {
        signal: Signal::fs_ffi_errors(),
        arity: Arity::Exact(1),
        doc: "Load the shared library at PATH, run its elle_plugin_init, and \
              return the struct of primitives the init builds. A later call with \
              the same path returns that struct and loads nothing. Requires :ffi, \
              because the init is foreign code.",
        params: &["path"],
        example: "(import/load-plugin \"target/release/libelle_regex.so\")",
        // Opaque, not Mixed: the path is copied out to a Rust String and never
        // retained, so no argument is stored. The result is the struct the
        // plugin's init built, which the cache holds, so it lives in neither
        // this call's region nor the path's (docs/impl/region/effects.md).
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
}
