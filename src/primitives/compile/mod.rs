//! Compiler-as-library primitives: analyze Elle source and query the results.
//!
//! The `compile/analyze` primitive runs the full analysis pipeline (reader →
//! expander → analyzer) and returns an opaque handle.  Other `compile/*`
//! primitives accept the handle and extract structured views: signals,
//! bindings, captures, call graph, diagnostics, symbols.

pub(super) mod query;
pub(super) mod transform;

use crate::primitives::def::RegionEffect;
use std::collections::{BTreeMap, BTreeSet, HashMap};

use crate::hir::BindingArena;
use crate::hir::{Binding, Hir, HirKind};
use crate::lint::diagnostics::{Diagnostic, Severity};
use crate::primitives::ctx::NativeCtx;
use crate::signals::registry::with_registry;
use crate::signals::Signal;
use crate::symbols::{SymbolDef, SymbolIndex, SymbolKind};
use crate::value::fiber::{SignalBits, SIG_ERROR, SIG_OK};
use crate::value::heap::TableKey;
use crate::value::types::Arity;
use crate::value::Value;

use query::prim_compile_analyze;
use query::prim_compile_binding;
use query::prim_compile_bindings;
use query::prim_compile_call_graph;
use query::prim_compile_callees;
use query::prim_compile_callers;
use query::prim_compile_captured_by;
use query::prim_compile_captures;
use query::prim_compile_diagnostics;
use query::prim_compile_primitives;
use query::prim_compile_query_signal;
use query::prim_compile_signal;
use query::prim_compile_symbols;
use transform::prim_compile_add_handler;
use transform::prim_compile_barrier_module;
use transform::prim_compile_dumps;
use transform::prim_compile_extract;
use transform::prim_compile_parallelize;
use transform::prim_compile_read_forms;
use transform::prim_compile_rename;
use transform::prim_compile_run_on;
use transform::prim_compile_whole_module;
use transform::prim_compile_whole_module_syntax;

mod convert;
pub(crate) use convert::*;

mod spans;
pub(crate) use spans::*;

mod callgraph;
pub(crate) use callgraph::*;

// ── Helper ─────────────────────────────────────────────────────────────

pub(super) fn kw(name: &str) -> TableKey {
    TableKey::Keyword(name.to_string())
}

// ── Analysis handle ────────────────────────────────────────────────────

/// Opaque handle wrapping the result of `analyze_file`.
///
/// Stored as `ctx.external("analysis", AnalysisHandle)`.  Query
/// primitives downcast the External to access the fields.
/// (byte_offset, byte_len) of a name token in source text.
pub(super) type NameSpan = (usize, usize);

pub struct AnalysisHandle {
    pub hir: Hir,
    pub arena: BindingArena,
    pub symbol_index: SymbolIndex,
    pub diagnostics: Vec<Diagnostic>,
    /// Function name → Signal, built eagerly.
    pub signal_map: HashMap<String, Signal>,
    /// Function name → `Vec<CallEdge>`, built eagerly.
    pub call_graph: CallGraphData,
    /// Original source text.
    pub source: String,
    /// Binding → all source locations where the binding's name appears.
    pub binding_spans: HashMap<Binding, Vec<NameSpan>>,
}

#[derive(Debug, Clone)]
pub struct CallEdge {
    pub callee: String,
    pub line: u32,
    pub col: u32,
    pub is_tail: bool,
}

#[derive(Debug, Clone, Default)]
pub struct CallGraphData {
    /// caller name → outgoing edges
    pub edges: HashMap<String, Vec<CallEdge>>,
    /// callee name → caller names
    pub reverse: HashMap<String, Vec<String>>,
    /// Functions with no callers.
    pub roots: Vec<String>,
    /// Functions that call no user-defined functions.
    pub leaves: Vec<String>,
}

// ── Signal map builder ─────────────────────────────────────────────────

// ── Call graph builder ─────────────────────────────────────────────────

// ── Binding spans builder ──────────────────────────────────────────────

// ── HIR search helpers ────────────────────────────────────────────────

// ── Value conversion helpers ───────────────────────────────────────────

// ── Extract the handle from an argument ────────────────────────────────

pub(super) fn get_handle<'a>(
    args: &'a [Value],
    name: &str,
    ctx: &mut NativeCtx,
) -> Result<&'a AnalysisHandle, (SignalBits, Value)> {
    match args[0].as_external::<AnalysisHandle>() {
        Some(h) => Ok(h),
        None => Err((
            SIG_ERROR,
            ctx.error(
                "type-error",
                format!(
                    "{}: expected analysis handle, got {}",
                    name,
                    args[0].type_name()
                ),
            ),
        )),
    }
}

/// Resolve a keyword argument to a function name string.
pub(super) fn resolve_name(
    args: &[Value],
    idx: usize,
    prim_name: &str,
    ctx: &mut NativeCtx,
) -> Result<String, (SignalBits, Value)> {
    // Accept keyword or string.
    if let Some(name) = args[idx].as_keyword_name() {
        return Ok(name.to_string());
    }
    if let Some(name) = args[idx].with_string(|s| s.to_string()) {
        return Ok(name);
    }
    Err((
        SIG_ERROR,
        ctx.error(
            "type-error",
            format!(
                "{}: expected keyword or string for function name, got {}",
                prim_name,
                args[idx].type_name()
            ),
        ),
    ))
}

// ── Registration ───────────────────────────────────────────────────────

primitive! {
    "compile/analyze" => prim_compile_analyze {
        signal: Signal::errors(),
        arity: Arity::Range(1, 2),
        doc: "Analyze Elle source text. Returns an opaque analysis handle for queries.",
        params: &["source", "opts"],
        category: "compile",
        example: r#"(compile/analyze "(defn f [x] (+ x 1))")"#,
        effect: RegionEffect::Fresh,
    }
    "compile/diagnostics" => prim_compile_diagnostics {
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return diagnostics (warnings, errors) from an analysis.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/diagnostics (compile/analyze src))"#,
        effect: RegionEffect::Fresh,
    }
    "compile/symbols" => prim_compile_symbols {
        signal: Signal::silent(),
        arity: Arity::Exact(1),
        doc: "Return all symbol definitions from an analysis.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/symbols (compile/analyze src))"#,
        effect: RegionEffect::Fresh,
    }
    "compile/signal" => prim_compile_signal {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return the inferred signal of a named function.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/signal analysis :my-fn)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/query-signal" => prim_compile_query_signal {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Find functions matching a signal query (:silent, :io, :yields, :jit-eligible, or signal name).",
        params: &["analysis", "query"],
        category: "compile",
        example: r#"(compile/query-signal analysis :silent)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/bindings" => prim_compile_bindings {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return all bindings from an analysis with metadata.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/bindings (compile/analyze src))"#,
        effect: RegionEffect::Fresh,
    }
    "compile/binding" => prim_compile_binding {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return detailed info about a specific binding.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/binding analysis :x)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/captures" => prim_compile_captures {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return what a function captures and how (value, lbox, transitive).",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/captures analysis :make-handler)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/captured-by" => prim_compile_captured_by {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return functions that capture the named binding.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/captured-by analysis :config)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/callers" => prim_compile_callers {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return functions that call the named function.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/callers analysis :fetch-page)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/callees" => prim_compile_callees {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Return functions called by the named function.",
        params: &["analysis", "name"],
        category: "compile",
        example: r#"(compile/callees analysis :main)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/call-graph" => prim_compile_call_graph {
        signal: Signal::errors(),
        arity: Arity::Exact(1),
        doc: "Return the full call graph with nodes, roots, and leaves.",
        params: &["analysis"],
        category: "compile",
        example: r#"(compile/call-graph (compile/analyze src))"#,
        effect: RegionEffect::Fresh,
    }
    "compile/primitives" => prim_compile_primitives {
        signal: Signal::silent(),
        arity: Arity::Exact(0),
        doc: "Return metadata for all Rust-defined primitives as an array of structs.",
        params: &[],
        category: "compile",
        example: r#"(compile/primitives)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/rename" => prim_compile_rename {
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Binding-aware rename. Returns new source with all references updated.",
        params: &["analysis", "old-name", "new-name"],
        category: "compile",
        example: r#"(compile/rename analysis :old-name :new-name)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/extract" => prim_compile_extract {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Extract a line range into a new function. Returns new source, captures, and signal.",
        params: &["analysis", "opts"],
        category: "compile",
        example: r#"(compile/extract analysis {:from :fn :lines [5 10] :name :new-fn})"#,
        effect: RegionEffect::Fresh,
    }
    "compile/parallelize" => prim_compile_parallelize {
        signal: Signal::errors(),
        arity: Arity::Exact(2),
        doc: "Check if functions can safely run in parallel. Verifies no shared mutable captures.",
        params: &["analysis", "fn-names"],
        category: "compile",
        example: r#"(compile/parallelize analysis [:fetch-a :fetch-b])"#,
        effect: RegionEffect::Fresh,
    }
    "compile/add-handler" => prim_compile_add_handler {
        signal: Signal::errors(),
        arity: Arity::Exact(3),
        doc: "Wrap call sites of a function with signal handling.",
        params: &["analysis", "fn-name", "signal-kind"],
        category: "compile",
        example: r#"(compile/add-handler analysis :fetch-page :error)"#,
        effect: RegionEffect::Fresh,
    }
    "compile/run-on" => prim_compile_run_on {
        signal: Signal::query_errors(),
        arity: Arity::AtLeast(2),
        doc: "Force-dispatch a closure on a specific tier (:bytecode, :jit, :mlir-cpu). Used by lib/differential.lisp to verify tier agreement. Returns the result, or signals :tier-rejected if the tier doesn't accept the closure.",
        params: &["tier", "f"],
        category: "compile",
        example: r#"(compile/run-on :bytecode (fn [a b] (+ a b)) 3 4)"#,
        // Re-enters the VM to run the caller's closure, so the RESULT is whatever
        // that closure returns — unbounded. The store side is not: the tier
        // dispatch reads the closure, and the Elle code it runs stores only
        // through the runtime-counted funnel, exactly as an opaque user fn does.
        // Unbounded result + no store is `Opaque` — no arg clique
        // (docs/impl/region/effects.md § Opaque).
        effect: RegionEffect::Opaque,
    }
    "compile/barrier-module" => prim_compile_barrier_module {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Compile a file (SOURCE, NAME) in the per-form fault-barrier test mode: whole-module analysis (epoch + shared bindings), then return a mutable array of [index thunk] pairs — one 0-arg thunk per test (expression) form, each capturing the file's shared bindings. def/var forms run eagerly as setup. Signals on a compile failure or a def-initializer fault. Powers `elle test` (src/test.lisp).",
        params: &["source", "name"],
        category: "compile",
        example: r#"(compile/barrier-module "(assert (= 1 1) \"ok\")" "<eval>")"#,
        // SOURCE and NAME are copied out to Rust `&str` by the front end, and the
        // setup thunk this runs is compiled FROM the source — it holds no
        // reference to either argument Value. So nothing is stored; what the
        // thunk run makes unbounded is the RESULT, which `result_minted` below
        // already accounts for at dispatch. `Opaque`, not `Mixed`
        // (docs/impl/region/effects.md § Opaque).
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
    "compile/whole-module" => prim_compile_whole_module {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Compile a file (SOURCE, NAME) as ONE whole-file thunk (legacy multi-form test mode): whole-module analysis (epoch + letrec bindings), then return a mutable array with a single [0 thunk] pair whose 0-arg thunk runs every top-level form (def/var and expressions alike) in source order. Unlike compile/barrier-module it does not hoist def/var eagerly or slice expressions per form — so an imperative script runs in order, once per tier, in isolation, matching a direct file run. Signals on a compile failure. Powers `elle test` for multi-form files (src/test.lisp).",
        params: &["source", "name"],
        category: "compile",
        example: r#"(compile/whole-module "(def x 1)\n(assert (= x 1) \"ok\")" "<eval>")"#,
        // `Opaque` for the same reason as `compile/barrier-module` above: both
        // arguments are copied out to `&str`, the thunk is compiled from the
        // source rather than from the argument Values, and only the result is
        // unbounded.
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
    "compile/read-forms" => prim_compile_read_forms {
        signal: Signal::of(SIG_OK.union(SIG_ERROR)),
        arity: Arity::Exact(2),
        doc: "Parse SOURCE (NAME for error spans) into a list of syntax values, without expanding or compiling. Companion to compile/whole-module-syntax: read a legacy multi-form file once in the main VM, then ship the syntax (sendable across os/spawn) to a worker that compiles + runs it with its own stdlib. Powers `elle test` (src/test.lisp).",
        params: &["source", "name"],
        category: "compile",
        example: r#"(compile/read-forms "(def x 1)\n(+ x 2)" "<eval>")"#,
        effect: RegionEffect::Fresh,
    }
    "compile/whole-module-syntax" => prim_compile_whole_module_syntax {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Like compile/whole-module, but from a list of already-parsed syntax values (FORMS, from compile/read-forms) plus NAME, instead of a source string. Returns a mutable array with one [0 thunk] pair. Lets the runner parse a multi-form file in the main VM and compile it in a worker against the worker's own stdlib. Powers `elle test` (src/test.lisp).",
        params: &["forms", "name"],
        category: "compile",
        example: r#"(compile/whole-module-syntax (compile/read-forms "(+ 1 2)" "<eval>") "<eval>")"#,
        // FORMS arrives as Values, and every one of them is CLONED into an owned
        // Rust `Syntax` before compilation (`dispatch_whole_module_syntax`), so no
        // argument Value reaches the compiled module. NAME is copied to `&str`.
        // Nothing stored, unbounded thunk-run result — `Opaque`.
        effect: RegionEffect::Opaque,
        result_minted: true,
    }
    "compile/dumps" => prim_compile_dumps {
        signal: Signal::query_errors(),
        arity: Arity::Exact(2),
        doc: "Compile a module (SOURCE, NAME) once through the file front-end and return a struct of rendered --dump artifacts keyed by kind: :ast :fhir :defuse :regions :hir :lir :cfg :dfa :jit :escape. The in-process form of `elle --dump=KIND` (returns the text instead of printing and exiting). Stages that fail to compile or yield nothing are omitted. Powers CAS asset capture in `elle test` (src/test.lisp).",
        params: &["source", "name"],
        category: "compile",
        example: r#"(compile/dumps "(+ 1 2)" "<eval>")"#,
        // Both arguments are copied out to `&str` and the artifacts are rendered
        // into fresh strings, so nothing is stored; the struct is minted by the
        // query dispatch rather than in this call's own region, so the result is
        // unbounded. `Opaque` — the table's clean face, pinned at 0 by
        // tests/elle/region-compile-clique-leak.lisp.
        effect: RegionEffect::Opaque,
    }
}
