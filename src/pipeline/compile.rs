//! Compilation pipeline: source -> bytecode.

use super::cache;
use super::CompileResult;
use crate::hir::tailcall::mark_tail_calls;
use crate::hir::{Analyzer, BindingArena, FileForm};
use crate::lir::{Emitter, Lowerer};
use crate::primitives::intern_primitive_names;
use crate::reader::{read_syntax, read_syntax_all_for};
use crate::symbol::SymbolTable;
use crate::syntax::{Span, Syntax, SyntaxKind};

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
    let (macro_vm_ptr, mut expander, meta) = cache::get_compilation_cache();
    // SAFETY: The cached VM is thread-local and pipeline functions are not
    // re-entrant. The RefCell borrow was released by get_compilation_cache.
    let macro_vm = unsafe { &mut *macro_vm_ptr };
    let expanded = expander.expand(syntax, symbols, macro_vm)?;

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

    // Phase 3.5: Mark tail calls
    mark_tail_calls(&mut analysis.hir);

    // Phase 4: Lower to LIR with intrinsic specialization
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone());
    let lir_func = lowerer.lower(&analysis.hir)?;

    // Phase 5: Emit bytecode with symbol names for cross-thread portability
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (bytecode, _yield_points, _call_sites) = emitter.emit(&lir_func);

    Ok(CompileResult {
        bytecode,
        warnings: Vec::new(),
    })
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

/// Compile a file as a single synthetic letrec.
///
/// All top-level forms are analyzed together, enabling mutual recursion.
/// Returns a single `CompileResult`. Primitives are pre-bound as immutable
/// Global bindings in an outer scope.
pub fn compile_file(
    source: &str,
    symbols: &mut SymbolTable,
    source_name: &str,
) -> Result<CompileResult, String> {
    intern_primitive_names(symbols);

    let mut syntaxes = read_syntax_all_for(source, source_name)?;

    // Phase 0: Epoch migration — rewrite old-epoch syntax before expansion
    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        crate::epoch::migrate_forms(&mut syntaxes, epoch)?;
    }

    let (macro_vm_ptr, mut expander, meta) = cache::get_compilation_cache();
    // SAFETY: The cached VM is thread-local and pipeline functions are not
    // re-entrant. The RefCell borrow was released by get_compilation_cache.
    let macro_vm = unsafe { &mut *macro_vm_ptr };

    // Expand all forms
    let mut expanded_forms = Vec::new();
    for syntax in syntaxes {
        let expanded = expander.expand(syntax, symbols, macro_vm)?;
        expanded_forms.push(expanded);
    }

    // Classify each form
    let forms: Vec<FileForm> = expanded_forms.iter().map(classify_form).collect();

    // Compute span covering all forms (or synthetic for empty)
    let span = if expanded_forms.is_empty() {
        Span::synthetic()
    } else {
        expanded_forms[0]
            .span
            .merge(&expanded_forms[expanded_forms.len() - 1].span)
    };

    // Analyze
    let mut arena = BindingArena::new();
    let mut analyzer = Analyzer::new_with_primitives(
        symbols,
        &mut arena,
        meta.signals.clone(),
        meta.arities.clone(),
    );
    analyzer.bind_primitives(&meta);
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let prim_values = analyzer.primitive_values().clone();
    drop(analyzer);

    // Mark tail calls
    mark_tail_calls(&mut hir);

    // Lower to LIR
    let intrinsics = crate::lir::intrinsics::build_intrinsics(symbols);
    let imm_prims = crate::lir::intrinsics::build_immediate_primitives(symbols);
    let symbol_names = symbols.all_names();
    let mut lowerer = Lowerer::new(&arena)
        .with_intrinsics(intrinsics)
        .with_immediate_primitives(imm_prims)
        .with_primitive_values(prim_values)
        .with_symbol_names(symbol_names.clone());
    let lir_func = lowerer.lower(&hir)?;

    // Emit bytecode
    let signal = lir_func.signal;
    let mut emitter = Emitter::new_with_symbols(symbol_names);
    let (mut bytecode, _, _) = emitter.emit(&lir_func);
    bytecode.signal = signal;

    Ok(CompileResult {
        bytecode,
        warnings: Vec::new(),
    })
}
