use super::*;
use crate::syntax::{ScopeId, SyntaxArena};

/// The fault-barrier transform (see `compile_barrier_module`). Runs on the
/// macro-EXPANDED top-level forms — so binding macros like `defn`/`def-` have
/// already become `def`/`var`, and are recognized as setup rather than wrapped:
///
/// ```text
/// prepend   (def __test-out (@array))
/// def/var   → unchanged (eager; establishes the shared binding)
/// signal    → unchanged (metadata; no entry)
/// expr E    → (push __test-out (array IDX (fn [] E)))   ; thunk, NOT run here
/// append    __test-out                                   ; module return value
/// ```
///
/// `IDX` is the form's position among the post-epoch, post-expansion forms,
/// which aligns with the runner's `read-all` index (used for label/hash/src).
/// Each test thunk is a closure capturing the shared bindings; it is created
/// (but not invoked) when the module runs on the bytecode tier, then run by the
/// runner on each tier. Setup forms run eagerly, so a later test form — and a
/// later thunk — sees earlier defs.
pub(super) fn barrier_transform(
    arena: &SyntaxArena,
    expanded: Vec<Syntax>,
    acc_scope: ScopeId,
) -> Vec<Syntax> {
    let sp = Span::synthetic();
    let sym = |s: &str| Syntax::symbol(arena, s, sp);
    let slist = |items: Vec<Syntax>| Syntax::list(arena, &items, sp);
    // The accumulator is compiler-injected, not written by the user. Scope-stamp
    // every occurrence with a fresh scope so it is *hygienic*: a reference the
    // user writes (which never carries this scope) can never resolve to it, so
    // `(environment)` excludes it — the same scope-subset rule that hides
    // macro-introduced bindings — and a test file that binds its own
    // `__test-out` does not collide with the harness.
    let acc = |s: &str| Syntax::symbol_scoped(arena, s, sp, &[acc_scope]);
    let acc_name = "__test-out";

    let mut out: Vec<Syntax> = Vec::with_capacity(expanded.len() + 2);
    // (def __test-out (@array))
    out.push(slist(vec![
        sym("def"),
        acc(acc_name),
        slist(vec![sym("@array")]),
    ]));

    for (idx, form) in expanded.into_iter().enumerate() {
        let is_setup = matches!(
            classify_form(&form),
            FileForm::Def(..) | FileForm::Var(..) | FileForm::Signal(..)
        );
        if is_setup {
            out.push(form);
        } else {
            // (fn [] E)
            let thunk = slist(vec![sym("fn"), Syntax::array(arena, &[], sp), form]);
            // (array IDX thunk)
            let pair = slist(vec![
                sym("array"),
                Syntax::new(SyntaxKind::Int(idx as i64), sp),
                thunk,
            ]);
            // (push __test-out pair)
            out.push(slist(vec![sym("push"), acc(acc_name), pair]));
        }
    }
    // __test-out  (the letrec body → the module's return value)
    out.push(acc(acc_name));
    out
}

/// The whole-file transform (see `compile_whole_module`). Runs on the
/// macro-EXPANDED top-level forms and wraps **all** of them — `def`/`var` setup
/// and expressions alike — into the body of a single 0-arg thunk, returned as
/// one `[0 thunk]` entry. This is the legacy multi-form path: a file is an
/// imperative script whose forms run in a load-bearing order, so it is run as one
/// unit (in source order, once per tier, in isolation) rather than sliced into
/// per-form thunks with `def`/`var` hoisted eagerly — which reorders the program
/// (read-before-write) and re-runs shared mutations per tier. The thunk body is
/// the internal `(%file-body form…)` form, NOT a bare `(fn () form…)` body: a
/// plain `fn` body uses internal-define semantics (all defines hoisted into one
/// frame, last-wins shadowing), whereas `%file-body` runs the exact
/// `analyze_file_letrec` a real file gets — so forward references, mutual
/// recursion, AND def redefinition (a redefinition's RHS sees the previous
/// binding) resolve identically to a direct `elle FILE` run. Output shape
/// matches `barrier_transform` (a `__test-out` array of `[index thunk]`) so the
/// runner consumes both the same way:
///
/// ```text
/// (def __test-out (@array))
/// (push __test-out (array 0 (fn () (%file-body form1 form2 … formN))))
/// __test-out
/// ```
pub(super) fn whole_module_transform(
    arena: &SyntaxArena,
    expanded: Vec<Syntax>,
    acc_scope: ScopeId,
) -> Vec<Syntax> {
    let sp = Span::synthetic();
    let sym = |s: &str| Syntax::symbol(arena, s, sp);
    let slist = |items: Vec<Syntax>| Syntax::list(arena, &items, sp);
    // Hygienic accumulator symbol (see `barrier_transform`).
    let acc = |s: &str| Syntax::symbol_scoped(arena, s, sp, &[acc_scope]);
    let acc_name = "__test-out";

    // (fn () (%file-body form1 … formN)) — all forms become the thunk body,
    // analyzed with file-scope letrec semantics. An empty file yields a thunk
    // whose body is nil.
    let body: Syntax = if expanded.is_empty() {
        sym("nil")
    } else {
        let mut fb: Vec<Syntax> = vec![sym("%file-body")];
        fb.extend(expanded);
        slist(fb)
    };
    let thunk = slist(vec![sym("fn"), Syntax::array(arena, &[], sp), body]);

    vec![
        // (def __test-out (@array))
        slist(vec![sym("def"), acc(acc_name), slist(vec![sym("@array")])]),
        // (push __test-out (array 0 thunk))
        slist(vec![
            sym("push"),
            acc(acc_name),
            slist(vec![
                sym("array"),
                Syntax::new(SyntaxKind::Int(0), sp),
                thunk,
            ]),
        ]),
        // __test-out  (the module's return value)
        acc(acc_name),
    ]
}

/// Compile a file as a single whole-file thunk (legacy multi-form path). Mirrors
/// `compile_barrier_module` but with `whole_module_transform`: the result is one
/// `[0 thunk]` entry whose thunk runs every form in source order. See
/// docs/test-runner.md § Multi-form files.
pub fn compile_whole_module(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CompileResult, String> {
    compile_module_with_transform(source, symbols, cctx, source_name, whole_module_transform)
}

/// Compile already-read top-level `syntaxes` as a single whole-file thunk
/// (`compile_whole_module` from syntax instead of source text). Powers
/// `compile/whole-module-syntax`: the test runner parses a legacy multi-form
/// file once in the main VM and ships the syntax to a worker, which compiles +
/// runs it with its own stdlib so the file's runtime `import`s and the worker's
/// `ev/run` scheduler agree on the dynamic scheduler parameters.
pub fn compile_whole_module_forms(
    syntaxes: Vec<Syntax>,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
) -> Result<CompileResult, String> {
    compile_syntaxes_with_transform(syntaxes, symbols, cctx, source_name, whole_module_transform)
}

/// Splice include/include-file directives in source text.
///
/// Reads top-level forms, resolves includes, and returns a single string
/// with all included content inlined. Used by the WASM backend to resolve
/// includes before wrapping user code in ev/run.
pub fn splice_includes(source: &str, source_name: &str) -> Result<String, String> {
    // Text in, text out: the trees exist only to be re-rendered, so they get
    // their own heap and die with it.
    let mut home = crate::syntax::SyntaxHeap::new();
    let arena = home.arena();
    let syntaxes = read_syntax_all_for(arena, source, source_name)?;
    let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
    let mut included: HashSet<String> = HashSet::from([source_name.to_string()]);
    let mut parts: Vec<String> = Vec::new();

    while let Some(syntax) = pending.pop_front() {
        if resolve_and_splice_include(&arena, &syntax, source_name, &mut pending, &mut included)? {
            continue;
        }
        parts.push(format!("{}", syntax));
    }

    Ok(parts.join("\n"))
}

/// Resolve and splice a single include directive into the pending queue.
/// Returns `Ok(true)` if the syntax was an include (resolved and spliced),
/// `Ok(false)` if it was not an include, or `Err` on resolution failure.
pub(super) fn resolve_and_splice_include(
    arena: &SyntaxArena,
    syntax: &Syntax,
    source_name: &str,
    pending: &mut std::collections::VecDeque<Syntax>,
    included: &mut HashSet<String>,
) -> Result<bool, String> {
    let (spec, is_include) = match extract_include(syntax) {
        Some(pair) => pair,
        None => return Ok(false),
    };
    let path = if is_include {
        crate::primitives::modules::resolve_import(&spec)
    } else {
        resolve_include_file(&spec, source_name)
    };
    let path = path.ok_or_else(|| format!("{}: include: '{}' not found", syntax.span, spec))?;
    if !included.insert(path.clone()) {
        return Err(format!(
            "{}: include: circular dependency on '{}'",
            syntax.span, path
        ));
    }
    let contents = std::fs::read_to_string(&path)
        .map_err(|e| format!("{}: include: failed to read '{}': {}", syntax.span, path, e))?;
    let forms = read_syntax_all_for(*arena, &contents, &path)?;
    for (i, form) in forms.into_iter().enumerate() {
        pending.insert(i, form);
    }
    Ok(true)
}

/// Extract the spec from `(include-file "path")` or `(include "spec")`.
/// Returns `(spec, is_include)` where `is_include` means use resolve_import.
pub(super) fn extract_include(syntax: &Syntax) -> Option<(String, bool)> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() == 2 {
            if let Some(head) = items[0].as_symbol() {
                let is_include = match head {
                    "include" => true,
                    "include-file" => false,
                    _ => return None,
                };
                if let SyntaxKind::String(s) = &items[1].kind {
                    return Some((s.to_string(), is_include));
                }
            }
        }
    }
    None
}

/// Resolve an include-file path relative to the including file's directory.
pub(super) fn resolve_include_file(spec: &str, source_name: &str) -> Option<String> {
    let base = std::path::Path::new(source_name).parent()?;
    let path = base.join(spec);
    if path.is_file() {
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    }
}

#[cfg(test)]
mod tests;
