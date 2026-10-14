// audited: 2026-10-14
//! `include` and `include-file`: resolve each to its file, and splice the file's forms in.
//!
//! docs/modules.md
//! src/pipeline/AGENTS.md

use super::*;
use crate::syntax::SyntaxArena;

/// Splice include/include-file directives in source text.
///
/// Reads top-level forms, resolves includes, and returns a single string
/// with all included content inlined. Used by the WASM backend to resolve
/// includes before wrapping user code in ev/run. An `include` resolves on the
/// macro VM of `cctx`, which must have the standard library loaded.
pub fn splice_includes(
    source: &str,
    source_name: &str,
    cctx: &mut CompileCtx,
) -> Result<String, String> {
    // Text in, text out: the trees exist only to be re-rendered, so they get
    // their own heap and die with it.
    let mut home = crate::syntax::SyntaxHeap::new();
    let arena = home.arena();
    let syntaxes = read_syntax_all_for(arena, source, source_name)?;
    let unit = crate::syntax::UnitTag::of(source_name, &syntaxes);
    cctx.with_macro_expansion(arena, Layer::Instance, unit, |vm, _expander, meta| {
        let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
        let mut included = included_root(source_name);
        let mut parts: Vec<String> = Vec::new();
        while let Some(syntax) = pending.pop_front() {
            if resolve_and_splice_include(&arena, &syntax, vm, &meta, &mut pending, &mut included)?
            {
                continue;
            }
            parts.push(format!("{}", syntax));
        }
        Ok(parts.join("\n"))
    })
}

/// The included-file set a compile starts from: the root file itself, when it
/// is a file, so a file that includes itself is a cycle.
pub(super) fn included_root(source_name: &str) -> HashSet<String> {
    let root = Span::synthetic().with_file(source_name).source_path();
    root.into_iter().collect()
}

/// Resolve and splice a single include directive into the pending queue.
/// Returns `Ok(true)` if the syntax was an include (resolved and spliced),
/// `Ok(false)` if it was not an include, or `Err` on resolution failure.
///
/// The including file is the one the form was read from, so an include inside
/// an included file resolves against that file. `(include spec)` names its file
/// through the standard library's `import/resolve`, called on `vm`, so that
/// `include` and `import` share one set of rules (docs/modules.md).
/// `(include-file path)` joins the including file's directory.
pub(super) fn resolve_and_splice_include(
    arena: &SyntaxArena,
    syntax: &Syntax,
    vm: &mut crate::vm::VM,
    meta: &crate::primitives::def::PrimitiveMeta,
    pending: &mut std::collections::VecDeque<Syntax>,
    included: &mut HashSet<String>,
) -> Result<bool, String> {
    let (spec, is_include) = match extract_include(syntax) {
        Some(pair) => pair,
        None => return Ok(false),
    };
    // An include reads a file for the compile, so it reads under the
    // capabilities of the fiber the compile runs for, as a macro does.
    if vm.fiber.withheld.intersects(crate::signals::SIG_FS) {
        return Err(format!(
            "{}: include: '{}' needs :fs, which the compiling fiber withholds",
            syntax.span, spec
        ));
    }
    let dir = syntax
        .span
        .source_path()
        .and_then(|f| crate::path::parent(&f).map(str::to_string));
    let path = if is_include {
        call_import_resolve(vm, meta, &spec, dir.as_deref())
            .map_err(|e| format!("{}: include: '{}' not found: {}", syntax.span, spec, e))?
    } else {
        resolve_include_file(&spec, dir.as_deref())
            .ok_or_else(|| format!("{}: include: '{}' not found", syntax.span, spec))?
    };
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

/// Call the standard library's `import/resolve` on `vm`: the absolute path of
/// the file `spec` names, given the including file's directory.
///
/// The arguments are born in a macro allocation scope, which the call closes
/// whatever it answers, as a macro expansion's scope closes
/// (docs/impl/region/rules.md).
fn call_import_resolve(
    vm: &mut crate::vm::VM,
    meta: &crate::primitives::def::PrimitiveMeta,
    spec: &str,
    dir: Option<&str>,
) -> Result<String, String> {
    let resolve = meta
        .functions
        .get(&crate::value::SymbolId::of("import/resolve"))
        .copied()
        .ok_or_else(|| "the standard library's import/resolve is not loaded".to_string())?;
    let heap = unsafe { &mut *vm.heap_ptr };
    let scope = crate::value::arena::begin_macro_scope(heap);
    let region = scope.arg_region();
    let args = [
        crate::value::build::string(heap, spec, region),
        dir.map_or(crate::value::Value::NIL, |d| {
            crate::value::build::string(heap, d, region)
        }),
    ];
    let answer = vm.call_closure(resolve, &args).and_then(|v| {
        v.with_string(|s| s.to_string())
            .ok_or_else(|| format!("import/resolve answered {}", v.type_name()))
    });
    crate::value::arena::reclaim_macro_scope(unsafe { &mut *vm.heap_ptr }, scope);
    answer
}

/// Extract the spec from `(include-file "path")` or `(include "spec")`.
/// Returns `(spec, is_include)`, where `is_include` means the spec resolves
/// through `import/resolve`.
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

/// Resolve an include-file path against the including file's directory, or
/// the working directory for code with no file. The answer is absolute and
/// normalized, the spelling the included set keys on.
pub(super) fn resolve_include_file(spec: &str, dir: Option<&str>) -> Option<String> {
    let joined = match dir {
        Some(dir) => crate::path::join(&[dir, spec]),
        None => spec.to_string(),
    };
    let path = crate::path::absolute(&joined).ok()?;
    crate::path::is_file(&path).then_some(path)
}

#[cfg(test)]
mod tests;
