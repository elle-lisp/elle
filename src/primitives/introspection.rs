//! Function introspection primitives

use crate::primitives::def::RegionEffect;
use crate::signals::Signal;
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK, SIG_QUERY};
use crate::value::types::Arity;
use crate::value::Value;

/// (jit? value) — true if closure has JIT-compiled code
pub(crate) fn prim_is_jit(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // SIG_QUERY to the VM, which checks jit_cache by bytecode pointer.
    (SIG_QUERY, ctx.pair(Value::keyword("jit?"), args[0]))
}

/// (silent? value) — true if closure is silent (does not suspend: no yield/debug/polymorphic)
pub(crate) fn prim_is_silent(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(!closure.signal().may_suspend()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (mutates-params? value) — true if closure mutates any parameters
pub(crate) fn prim_mutates_params(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        (
            SIG_OK,
            Value::bool(closure.template.capture_params_mask != 0),
        )
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (fn/gpu-eligible? value) — true if closure is eligible for GPU compilation
pub(crate) fn prim_gpu_eligible(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        let eligible = match &closure.template.lir_function {
            Some(lir) => lir.is_gpu_eligible(),
            None => closure.template.is_gpu_candidate(),
        };
        (SIG_OK, Value::bool(eligible))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (fn/errors? value) — true if closure may error
pub(crate) fn prim_errors(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::bool(closure.template.signal.may_error()))
    } else {
        (SIG_OK, Value::FALSE)
    }
}

/// (fiber? val) → bool
///
/// Returns true if the value is a fiber.
pub(crate) fn prim_is_fiber(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    (SIG_OK, Value::bool(args[0].is_fiber()))
}

/// (arity value) — closure arity as int, pair, or nil
pub(crate) fn prim_arity(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        let result = match closure.template.arity {
            Arity::Exact(n) => Value::int(n as i64),
            Arity::AtLeast(n) => ctx.pair(Value::int(n as i64), Value::NIL),
            Arity::Range(min, max) => ctx.pair(Value::int(min as i64), Value::int(max as i64)),
        };
        (SIG_OK, result)
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (captures value) — number of captured variables, or nil
pub(crate) fn prim_captures(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::int(closure.env.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (bytecode-size value) — size of bytecode in bytes, or nil
pub(crate) fn prim_bytecode_size(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(closure) = args[0].as_closure() {
        (SIG_OK, Value::int(closure.template.bytecode.len() as i64))
    } else {
        (SIG_OK, Value::NIL)
    }
}

/// (doc target) — look up documentation for a closure, primitive, or special form.
///
/// Dispatch:
/// - closure value → returns `closure.template.doc` (the leading string literal
///   from the function body), or "No documentation found for 'name'" if absent.
/// - string or keyword → sends SIG_QUERY "doc" to the VM, which looks up
///   `vm.docs`. Only native primitives and special forms are in `vm.docs`;
///   stdlib functions are NOT (their docstrings live in the closure value).
///
/// Usage: prefer `(doc name)` over `(doc "name")`. The analyzer rewrites
/// `(doc name)` appropriately: closures are passed through as values; native
/// primitives and special forms are rewritten to `(doc "name")` string lookup.
/// Passing an explicit string `(doc "stdlib-fn")` will NOT find stdlib docs.
pub(crate) fn prim_doc(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    // Closure: extract docstring directly — no VM query needed.
    if let Some(closure) = args[0].as_closure() {
        return if let Some(doc) = closure.template.doc.as_deref() {
            // The docstring is plain `Rc<str>` template data — materialize a
            // FRESH ordinary string in the active region, as any native-fn
            // result does.
            (SIG_OK, ctx.string(doc))
        } else {
            let name = closure.template.name.as_deref().unwrap_or("<anonymous>");
            (
                SIG_OK,
                ctx.string(format!("No documentation found for '{}'", name)),
            )
        };
    }
    // String or keyword: look up builtin docs via SIG_QUERY.
    (SIG_QUERY, ctx.pair(Value::keyword("doc"), args[0]))
}

/// (vm/query op arg) — query VM state
///
/// The single gateway to SIG_QUERY. `op` is a string or keyword
/// naming the operation; `arg` is the operation-specific argument.
/// The VM's dispatch_query handles the rest.
///
/// Operations:
/// - "call-count" closure → int
/// - "doc" name → string
/// - "global?" symbol → bool
/// - "fiber/self" _ → fiber or nil
pub(crate) fn prim_vm_query(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if !args[0].is_string() && args[0].as_keyword_name().is_none() {
        return (
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "vm/query: operation must be a string or keyword, got {}",
                    args[0].type_name()
                ),
            ),
        );
    }
    (SIG_QUERY, ctx.pair(args[0], args[1]))
}

/// (signals) — return the signal registry as a struct mapping keywords to bit positions
pub(crate) fn prim_signals(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    let reg = crate::signals::registry::global_registry().lock().unwrap();
    let mut map = std::collections::BTreeMap::new();
    for entry in reg.entries() {
        let key = crate::value::TableKey::from_value(&Value::keyword(&entry.name)).unwrap();
        map.insert(key, Value::int(entry.bit_position as i64));
    }
    (SIG_OK, ctx.struct_from(map))
}

/// (keyword str) — convert a string to a keyword
///
/// Creates a content-addressed keyword from the string name.
pub(crate) fn prim_keyword(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if let Some(kw) = args[0].with_string(Value::keyword) {
        (SIG_OK, kw)
    } else {
        type_error!(ctx, args[0], "keyword", "string")
    }
}

/// (lir/closure-value-const-count) — number of closure-valued `ValueConst`
/// instructions converted to `ClosureRef` by the LIR cross-thread
/// serializer during this process's lifetime.
///
/// Used by regression tests to assert the ClosureRef LIR-transfer fix
/// is actually firing on real spawn patterns. See
/// `src/lir/types.rs::convert_value_consts_for_send`.
pub(crate) fn prim_closure_value_const_count(
    _ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_OK,
        Value::int(crate::lir::closure_value_const_count() as i64),
    )
}

/// (jit/rejections) — list closures rejected from JIT compilation with reasons
///
/// Returns a list of structs, each with :name, :reason, and :calls keys.
/// Sorted by call count ascending (coldest first).
pub(crate) fn prim_jit_rejections(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    _args: &[Value],
) -> (SignalBits, Value) {
    (
        SIG_QUERY,
        ctx.pair(Value::keyword("jit/rejections"), Value::NIL),
    )
}

/// (mlir/compile-spirv closure [workgroup-size]) — compile closure to SPIR-V bytes
#[cfg(feature = "mlir")]
pub(crate) fn prim_compile_spirv(
    ctx: &mut crate::primitives::ctx::NativeCtx<'_>,
    args: &[Value],
) -> (SignalBits, Value) {
    if args.is_empty() || args.len() > 2 {
        return (
            SIG_ERROR,
            ctx.error(
                "arity-error",
                format!(
                    "mlir/compile-spirv: expected 1-2 arguments, got {}",
                    args.len()
                ),
            ),
        );
    }
    let closure = prim_arg!(ctx, args, 0, as_closure, "mlir/compile-spirv", "closure");
    let lir = match &closure.template.lir_function {
        Some(lir) => lir,
        None => {
            return (
                SIG_ERROR,
                ctx.error(
                    "mlir-error",
                    "mlir/compile-spirv: closure has no LIR".to_string(),
                ),
            )
        }
    };
    if !lir.is_gpu_eligible() {
        return (
            SIG_ERROR,
            ctx.error(
                "mlir-error",
                "mlir/compile-spirv: closure is not GPU-eligible".to_string(),
            ),
        );
    }
    let workgroup_size = if args.len() == 2 {
        args[1].as_int().unwrap_or(256) as u32
    } else {
        256
    };
    // Use SIG_QUERY to access the VM's MlirCache for shared context
    // and SPIR-V caching. The VM handles the query in dispatch_query.
    let payload = ctx.pair(args[0], Value::int(workgroup_size as i64));
    (
        SIG_QUERY,
        ctx.pair(Value::keyword("mlir/compile-spirv"), payload),
    )
}

// Declarative primitive definitions for introspection operations.
primitive! {
    "jit?" => prim_is_jit {
        signal: Signal::query_errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure has JIT-compiled code",
        params: &["value"],
        category: "predicate",
        example: "(jit? (fn (x) x))",
        effect: RegionEffect::Immediate,
    }
    "silent?" => prim_is_silent {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure is silent (does not suspend: no yield, debug, or polymorphic signal). False for non-closures.",
        params: &["value"],
        category: "predicate",
        example: "(silent? (fn (x) x))",
        effect: RegionEffect::Immediate,
    }
    "fiber?" => prim_is_fiber {
        arity: Arity::Exact(1),
        doc: "Returns true if value is a fiber",
        params: &["value"],
        category: "predicate",
        example: "(fiber? (fiber/new (fn () 42) |:yield|))",
        effect: RegionEffect::Immediate,
    }
    "fn/mutates-params?" => prim_mutates_params {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure mutates any parameters",
        params: &["value"],
        category: "fn",
        example: "(fn/mutates-params? (fn (x) (assign x 1)))",
        aliases: &["mutates-params?"],
        effect: RegionEffect::Immediate,
    }
    "fn/gpu-eligible?" => prim_gpu_eligible {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure passes signal and structural checks for GPU compilation",
        params: &["value"],
        category: "fn",
        example: "(fn/gpu-eligible? (fn [a b] (+ a b)))",
        effect: RegionEffect::Immediate,
    }
    "fn/errors?" => prim_errors {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns true if closure may error",
        params: &["value"],
        category: "fn",
        example: "(fn/errors? (fn (x) (/ 1 x)))",
        effect: RegionEffect::Immediate,
    }
    "fn/arity" => prim_arity {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns closure arity as int, pair, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/arity (fn (x y) x))",
        aliases: &["arity"],
        effect: RegionEffect::Fresh,
    }
    "fn/captures" => prim_captures {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns number of captured variables, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/captures (let [x 1] (fn () x)))",
        aliases: &["captures"],
        effect: RegionEffect::Immediate,
    }
    "fn/bytecode-size" => prim_bytecode_size {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Returns size of bytecode in bytes, or nil",
        params: &["value"],
        category: "fn",
        example: "(fn/bytecode-size (fn (x) x))",
        aliases: &["bytecode-size"],
        effect: RegionEffect::Immediate,
    }
    "doc" => prim_doc {
        signal: Signal::query_errors(),
        arity: Arity::Exact(1),
        doc: "Look up documentation for a value or builtin. \
              Pass a closure (user-defined or stdlib) to extract its docstring. \
              Pass a string or keyword to look up a native primitive or special form by name. \
              Note: (doc name) works for closures and native primitives; \
              (doc \"name\") only works for native primitives and special forms.",
        params: &["target"],
        category: "meta",
        example: "(doc inc)",
        effect: RegionEffect::Fresh,
    }
    "vm/query" => prim_vm_query {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Query VM state (call-count, doc, global?, fiber/self)",
        params: &["op", "arg"],
        category: "meta",
        example: "(vm/query \"call-count\" some-fn)",
        // The gateway picks its operation by a runtime string, so the RESULT is
        // unbounded — minted by whatever `VM::dispatch_query` reached. The store
        // side is not: every operation there reads its argument or copies it out,
        // and the Elle code some of them re-enter stores only through the
        // runtime-counted funnel. Unbounded result + no store is `Opaque` — no arg
        // clique (docs/impl/region/effects.md § Opaque;
        // tests/elle/region-query-clique-leak.lisp). The obligation rides
        // `dispatch_query`: an operation that RETAINS its argument past the call
        // moves this declaration back to `Mixed`.
        effect: RegionEffect::Opaque,
    }
    "signals" => prim_signals {
        signal: Signal::errors(),
        doc: "Return the signal registry as a struct mapping keywords to bit positions.",
        category: "meta",
        example: "(signals)",
        effect: RegionEffect::Fresh,
    }
    "jit/rejections" => prim_jit_rejections {
        signal: Signal::query_errors(),
        doc: "List closures rejected from JIT compilation. Returns list of {:name :reason :calls} structs sorted by call count ascending.",
        category: "meta",
        example: "(jit/rejections)",
        effect: RegionEffect::Fresh,
    }
    "lir/closure-value-const-count" => prim_closure_value_const_count {
        doc: "Number of closure-valued ValueConst instructions converted to ClosureRef by the LIR cross-thread serializer. Used by regression tests to assert the ClosureRef LIR-transfer fix fires.",
        category: "meta",
        example: "(lir/closure-value-const-count)",
        effect: RegionEffect::Immediate,
    }
    "keyword" => prim_keyword {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Convert a string to a keyword.",
        params: &["str"],
        category: "conversion",
        example: "(keyword \"foo\")",
        aliases: &["string->keyword"],
        effect: RegionEffect::Immediate,
    }
}

#[cfg(feature = "mlir")]
primitive!(
    pub(crate) static MLIR_PRIMITIVES =
        "mlir/compile-spirv" => prim_compile_spirv {
            signal: Signal::query_errors(),
            arity: Arity::Range(1, 2),
            doc: "Compile a GPU-eligible closure to SPIR-V bytes.",
            params: &["closure", "workgroup-size"],
            category: "mlir",
            example: "(mlir/compile-spirv (fn [a b] (+ a b)))",
            effect: RegionEffect::Fresh,
        }
);
