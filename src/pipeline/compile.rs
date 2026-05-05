//! Compilation pipeline: source -> bytecode.

use super::cache;
use super::CompileResult;
use crate::hir::functionalize::functionalize;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena, FileForm};
use crate::lir::{Emitter, Lowerer};
use crate::primitives::intern_primitive_names;
use crate::reader::{read_syntax, read_syntax_all_for};
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};
use std::collections::HashSet;

/// Compile source code to LIR (for the WASM backend).
///
/// Runs phases 1-4 (parse, expand, analyze, lower) and returns the
/// LirModule before bytecode emission.
pub fn compile_to_lir(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<crate::lir::LirModule, String> {
    intern_primitive_names(symbols);

    let syntax = read_syntax(source, source_name)?;

    let (expanded, meta) = cache::with_compilation_cache(|macro_vm, mut expander, meta| {
        let expanded = expander.expand(syntax, symbols, macro_vm)?;
        Ok::<_, String>((expanded, meta))
    })?;

    let mut arena = crate::hir::BindingArena::new();
    let mut analyzer = crate::hir::Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded)?;
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);

    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::typeinfer::infer_and_rewrite(&mut analysis.hir, &arena, symbols);

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols);
    let region_info =
        crate::hir::analyze_regions_with(&analysis.hir, &arena, pc.call_classification.clone());
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names)
        .with_region_info(region_info);
    let result = lowerer.lower(&analysis.hir);
    crate::lir::lower::accumulate_scope_stats(lowerer.scope_stats());
    result
}

/// Compile source code to bytecode.
///
/// Creates an internal VM for macro expansion. Macro side effects
/// don't persist beyond compilation.
pub fn compile(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<CompileResult, String> {
    // Ensure caller's SymbolTable has primitive names interned so that
    // SymbolIds match the cached PrimitiveMeta.
    intern_primitive_names(symbols);

    // Phase 1: Parse to Syntax
    let syntax = read_syntax(source, source_name)?;

    // Phase 2: Macro expansion (cached VM for macro bodies)
    let (expanded, meta) = cache::with_compilation_cache(|macro_vm, mut expander, meta| {
        let expanded = expander.expand(syntax, symbols, macro_vm)?;
        Ok::<_, String>((expanded, meta))
    })?;

    // Phase 3: Analyze to HIR with interprocedural signal and arity tracking
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    let mut analysis = analyzer.analyze(&expanded)?;
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);

    // Phase 3.5: Mark tail calls + functionalize + type inference
    mark_tail_calls(&mut analysis.hir);
    functionalize(&mut analysis.hir, &mut arena);
    crate::hir::typeinfer::infer_and_rewrite(&mut analysis.hir, &arena, symbols);

    // Phase 4: Lower to LIR with intrinsic specialization
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols);
    let region_info =
        crate::hir::analyze_regions_with(&analysis.hir, &arena, pc.call_classification.clone());
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone())
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&analysis.hir)?;

    // Phase 5: Emit bytecode with symbol names for cross-thread portability
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    Ok(CompileResult { bytecode })
}

/// Classify an expanded top-level form into a `FileForm`.
///
/// - `(def name value)` → `FileForm::Def(name, value)`
/// - `(var name value)` → `FileForm::Var(name, value)`
/// - anything else → `FileForm::Expr(syntax)`
pub(super) fn classify_form(syntax: &Syntax) -> FileForm<'_> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() == 2 {
            if let Some(head) = items[0].as_symbol() {
                if head == "signal" {
                    return FileForm::Signal(&items[1]);
                }
            }
        }
        if items.len() == 3 {
            if let Some(head) = items[0].as_symbol() {
                match head {
                    "def" => return FileForm::Def(&items[1], &items[2]),
                    "var" => return FileForm::Var(&items[1], &items[2]),
                    _ => {}
                }
            }
        }
    }
    FileForm::Expr(syntax)
}

/// Compile a file to LIR as a single synthetic letrec (for WASM backend).
///
/// `epoch_skip` — number of leading forms to exclude from epoch migration
/// (e.g. stdlib forms that are already in the current epoch). When 0,
/// epoch migration applies to all forms.
pub fn compile_file_to_lir(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
    epoch_skip: usize,
) -> Result<crate::lir::LirModule, String> {
    intern_primitive_names(symbols);

    let mut syntaxes = read_syntax_all_for(source, source_name)?;

    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        if epoch_skip > 0 && epoch_skip < syntaxes.len() {
            crate::epoch::migrate_forms(&mut syntaxes[epoch_skip..], epoch)?;
        } else {
            crate::epoch::migrate_forms(&mut syntaxes, epoch)?;
        }
    }

    // Expand all forms, splicing include/include-file inline
    let (expanded_forms, meta) = cache::with_compilation_cache(|macro_vm, mut expander, meta| {
        let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
        let mut expanded_forms = Vec::new();
        let mut included: HashSet<String> = HashSet::from([source_name.to_string()]);
        while let Some(syntax) = pending.pop_front() {
            if let Some((spec, is_include)) = extract_include(&syntax) {
                let path = if is_include {
                    crate::primitives::modules::resolve_import(&spec)
                } else {
                    resolve_include_file(&spec, source_name)
                };
                let path =
                    path.ok_or_else(|| format!("{}: include: '{}' not found", syntax.span, spec))?;
                if !included.insert(path.clone()) {
                    return Err(format!(
                        "{}: include: circular dependency on '{}'",
                        syntax.span, path
                    ));
                }
                let contents = std::fs::read_to_string(&path).map_err(|e| {
                    format!("{}: include: failed to read '{}': {}", syntax.span, path, e)
                })?;
                let forms = read_syntax_all_for(&contents, &path)?;
                for (i, form) in forms.into_iter().enumerate() {
                    pending.insert(i, form);
                }
                continue;
            }
            let expanded = expander.expand(syntax, symbols, macro_vm)?;
            expanded_forms.push(expanded);
        }
        Ok((expanded_forms, meta))
    })?;

    let forms: Vec<FileForm> = expanded_forms.iter().map(classify_form).collect();

    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    let effective_epoch = source_epoch.unwrap_or(crate::epoch::CURRENT_EPOCH);
    analyzer.set_immutable_by_default(effective_epoch >= 8);
    analyzer.bind_primitives(&meta);
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let prim_values = analyzer.primitive_values().clone();
    let errors = analyzer.take_errors();
    drop(analyzer);

    // If analysis accumulated any recoverable errors (undefined vars,
    // signal mismatches, etc.), surface the first one now in the standard
    // "file:line:col: message" format. Without this, poison nodes from
    // accumulated errors reach the lowerer and surface as the opaque
    // "internal: error poison node in lowerer".
    if !errors.is_empty() {
        let err = &errors[0];
        let msg = match &err.location {
            Some(loc) => format!(
                "{}:{}:{}: {}",
                loc.file,
                loc.line,
                loc.col,
                err.description()
            ),
            None => err.description(),
        };
        return Err(msg);
    }

    mark_tail_calls(&mut hir);
    functionalize(&mut hir, &mut arena);
    crate::hir::typeinfer::infer_and_rewrite(&mut hir, &arena, symbols);

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols);
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names)
        .with_region_info(region_info);
    let result = lowerer.lower(&hir);
    crate::lir::lower::accumulate_scope_stats(lowerer.scope_stats());
    result
}

/// Shared front-end for file compilation: parse, epoch migration, macro
/// expansion (with file-scope stamping and include resolution), analysis,
/// error surfacing, tail-call marking, and functionalization.
///
/// Returns `(hir, arena, expander, prim_values, signal_projection)`.
/// Callers that don't need all fields can ignore the extras.
#[allow(clippy::type_complexity)]
fn compile_file_frontend(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<
    (
        crate::hir::Hir,
        BindingArena,
        crate::syntax::Expander,
        std::collections::HashMap<crate::hir::Binding, crate::value::Value>,
        Option<std::collections::HashMap<String, crate::signals::Signal>>,
    ),
    String,
> {
    intern_primitive_names(symbols);

    let mut syntaxes = read_syntax_all_for(source, source_name)?;
    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        crate::epoch::migrate_forms(&mut syntaxes, epoch)?;
    }

    let (expanded_forms, expander, meta) =
        cache::with_compilation_cache(|macro_vm, mut expander, meta| {
            let mut pending: std::collections::VecDeque<Syntax> = if source_name.starts_with('<') {
                syntaxes.into()
            } else {
                let file_scope = expander.fresh_scope();
                syntaxes
                    .into_iter()
                    .map(|s| expander.stamp_scope(s, file_scope))
                    .collect()
            };
            let mut expanded_forms = Vec::new();
            let mut included: HashSet<String> = HashSet::from([source_name.to_string()]);
            while let Some(syntax) = pending.pop_front() {
                if let Some((spec, is_include)) = extract_include(&syntax) {
                    let path = if is_include {
                        crate::primitives::modules::resolve_import(&spec)
                    } else {
                        resolve_include_file(&spec, source_name)
                    };
                    let path = path
                        .ok_or_else(|| format!("{}: include: '{}' not found", syntax.span, spec))?;
                    if !included.insert(path.clone()) {
                        return Err(format!(
                            "{}: include: circular dependency on '{}'",
                            syntax.span, path
                        ));
                    }
                    let contents = std::fs::read_to_string(&path).map_err(|e| {
                        format!("{}: include: failed to read '{}': {}", syntax.span, path, e)
                    })?;
                    let forms = read_syntax_all_for(&contents, &path)?;
                    for (i, form) in forms.into_iter().enumerate() {
                        pending.insert(i, form);
                    }
                    continue;
                }
                let expanded = expander.expand(syntax, symbols, macro_vm)?;
                expanded_forms.push(expanded);
            }
            Ok((expanded_forms, expander, meta))
        })?;

    let forms: Vec<FileForm> = expanded_forms.iter().map(classify_form).collect();
    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    let effective_epoch = source_epoch.unwrap_or(crate::epoch::CURRENT_EPOCH);
    analyzer.set_immutable_by_default(effective_epoch >= 8);
    analyzer.bind_primitives(&meta);
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let prim_values = analyzer.primitive_values().clone();
    let signal_projection = analyzer.take_signal_projection();
    let errors = analyzer.take_errors();
    drop(analyzer);

    if !errors.is_empty() {
        let err = &errors[0];
        let msg = match &err.location {
            Some(loc) => format!(
                "{}:{}:{}: {}",
                loc.file,
                loc.line,
                loc.col,
                err.description()
            ),
            None => err.description(),
        };
        return Err(msg);
    }

    mark_tail_calls(&mut hir);
    functionalize(&mut hir, &mut arena);
    crate::hir::typeinfer::infer_and_rewrite(&mut hir, &arena, symbols);

    Ok((hir, arena, expander, prim_values, signal_projection))
}

/// Compile a file as a single synthetic letrec.
///
/// Compile to functionalized HIR (for `--dump=fhir`). Returns the HIR tree,
/// the binding arena, and the symbol name map.
pub fn compile_file_to_fhir(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<
    (
        crate::hir::Hir,
        BindingArena,
        std::collections::HashMap<u32, String>,
    ),
    String,
> {
    let (hir, arena, _expander, _prim_values, _signal_projection) =
        compile_file_frontend(source, symbols, source_name)?;
    let names = symbols.all_names();
    Ok((hir, arena, names))
}

/// All top-level forms are analyzed together, enabling mutual recursion.
/// Returns a single `CompileResult`. Primitives are pre-bound as immutable
/// Global bindings in an outer scope.
pub fn compile_file(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<CompileResult, String> {
    compile_file_inner(source, symbols, source_name).map(|(result, _)| result)
}

/// Like `compile_file`, but also returns the Expander after expansion.
/// The REPL uses this to persist macro definitions across inputs.
pub fn compile_file_repl(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<(CompileResult, crate::syntax::Expander), String> {
    compile_file_inner(source, symbols, source_name)
}

fn compile_file_inner(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<(CompileResult, crate::syntax::Expander), String> {
    let (hir, arena, expander, prim_values, signal_projection) =
        compile_file_frontend(source, symbols, source_name)?;

    // Lower to LIR
    let pc = crate::lir::intrinsics::PrimitiveClassification::new(symbols);
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone())
        .with_region_info(region_info);
    let lir_module = lowerer.lower(&hir)?;

    // Emit bytecode
    let signal = lir_module.entry.signal;
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (mut bytecode, _, _) = emitter.emit_module(&lir_module);
    bytecode.signal = signal;
    bytecode.signal_projection = signal_projection;

    Ok((CompileResult { bytecode }, expander))
}

/// Splice include/include-file directives in source text.
///
/// Reads top-level forms, resolves includes, and returns a single string
/// with all included content inlined. Used by the WASM backend to resolve
/// includes before wrapping user code in ev/run.
pub fn splice_includes(source: &str, source_name: &str) -> Result<String, String> {
    let syntaxes = read_syntax_all_for(source, source_name)?;
    let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
    let mut included: HashSet<String> = HashSet::from([source_name.to_string()]);
    let mut parts: Vec<String> = Vec::new();

    while let Some(syntax) = pending.pop_front() {
        if let Some((spec, is_include)) = extract_include(&syntax) {
            let path = if is_include {
                crate::primitives::modules::resolve_import(&spec)
            } else {
                resolve_include_file(&spec, source_name)
            };
            let path =
                path.ok_or_else(|| format!("{}: include: '{}' not found", syntax.span, spec))?;
            if !included.insert(path.clone()) {
                return Err(format!(
                    "{}: include: circular dependency on '{}'",
                    syntax.span, path
                ));
            }
            let contents = std::fs::read_to_string(&path).map_err(|e| {
                format!("{}: include: failed to read '{}': {}", syntax.span, path, e)
            })?;
            let forms = read_syntax_all_for(&contents, &path)?;
            for (i, form) in forms.into_iter().enumerate() {
                pending.insert(i, form);
            }
            continue;
        }
        // Preserve the original source text for this form
        parts.push(format!("{}", syntax));
    }

    Ok(parts.join("\n"))
}

/// Extract the spec from `(include-file "path")` or `(include "spec")`.
/// Returns `(spec, is_include)` where `is_include` means use resolve_import.
fn extract_include(syntax: &Syntax) -> Option<(String, bool)> {
    if let SyntaxKind::List(items) = &syntax.kind {
        if items.len() == 2 {
            if let Some(head) = items[0].as_symbol() {
                let is_include = match head {
                    "include" => true,
                    "include-file" => false,
                    _ => return None,
                };
                if let SyntaxKind::String(s) = &items[1].kind {
                    return Some((s.clone(), is_include));
                }
            }
        }
    }
    None
}

/// Resolve an include-file path relative to the including file's directory.
fn resolve_include_file(spec: &str, source_name: &str) -> Option<String> {
    let base = std::path::Path::new(source_name).parent()?;
    let path = base.join(spec);
    if path.is_file() {
        Some(path.to_string_lossy().into_owned())
    } else {
        None
    }
}
