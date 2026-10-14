// audited: 2026-10-14
//! Compile a file to frozen LIR, for the WASM backend and the dumps.
//!
//! src/pipeline/AGENTS.md
//! docs/impl/wasm.md

use super::*;

/// Compile a file to frozen LIR as a single synthetic letrec (for the WASM
/// backend and the dumps).
///
/// `epoch_skip` — number of leading forms to exclude from epoch migration
/// (for example, stdlib forms already in the current epoch). When 0, epoch
/// migration applies to all forms.
pub fn compile_file_to_lir(
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    epoch_skip: usize,
) -> Result<crate::lir::FrozenModule, String> {
    with_syntax_arena(cctx.heap_ptr(), |arena| {
        compile_file_to_lir_inner(arena, source, symbols, cctx, source_name, epoch_skip)
    })
}

/// Read, expand and analyze a file as one letrec, then lower it.
fn compile_file_to_lir_inner(
    arena: SyntaxArena,
    source: &str,
    symbols: &mut SymbolTable,
    cctx: &mut CompileCtx,
    source_name: &str,
    epoch_skip: usize,
) -> Result<crate::lir::FrozenModule, String> {
    let mut syntaxes = read_syntax_all_for(arena, source, source_name)?;
    crate::epoch::check_lexicon_agreement(&syntaxes, source, source_name)?;
    let unit = UnitTag::of(source_name, &syntaxes);

    let source_epoch = crate::epoch::extract_epoch(&mut syntaxes)?;
    crate::pipeline::directives::extract_semver_directives(&mut syntaxes)?;
    if let Some(epoch) = source_epoch {
        if epoch_skip > 0 && epoch_skip < syntaxes.len() {
            crate::epoch::migrate_forms(&arena, &mut syntaxes[epoch_skip..], epoch)?;
        } else {
            crate::epoch::migrate_forms(&arena, &mut syntaxes, epoch)?;
        }
    }

    // Expand all forms, splicing include/include-file inline
    let (expanded_forms, meta, core_env) = cctx.with_macro_expansion(
        arena,
        Layer::Instance,
        unit,
        |macro_vm, mut expander, meta| {
            let mut pending: std::collections::VecDeque<Syntax> = syntaxes.into();
            let mut expanded_forms = Vec::new();
            let mut includes = Includes::root(source_name);
            while let Some(syntax) = pending.pop_front() {
                if resolve_and_splice_include(
                    &arena,
                    &syntax,
                    macro_vm,
                    &meta,
                    &mut pending,
                    &mut includes,
                )? {
                    continue;
                }
                expanded_forms.push(expander.expand(syntax, symbols, macro_vm)?);
            }
            Ok::<_, String>((expanded_forms, meta, expander.core_env.clone()))
        },
    )?;

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
    analyzer.set_unicode_generation(cctx.unicode_generation());
    analyzer.bind_primitives(&meta);
    if !core_env.is_empty() {
        analyzer.bind_compile_time_env(&core_env, true);
    }
    let mut hir = analyzer.analyze_file_letrec(forms, span)?;
    let prim_values = analyzer.primitive_values().clone();
    let errors = analyzer.take_errors();
    drop(analyzer);

    // If analysis collected recoverable errors, such as an undefined variable
    // or a signal mismatch, report the first one as "file:line:col: message".
    // Without this, the poison nodes those errors left reach the lowerer and
    // report the opaque "internal: error poison node in lowerer".
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

    let types = crate::hir::regularize(&mut hir, &mut arena, symbols, &prim_values, cctx.graft())?;

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(cctx.primitive_meta());
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        eprintln!(
            "[trace:regions] compile_file_to_lir:\n{}",
            crate::hir::format_regions(&region_info, &arena, Some(symbols))
        );
    }
    let mut lowerer = Lowerer::new(&arena)
        .with_heap(unsafe { &mut *cctx.heap_ptr() })
        .with_symbols(symbols)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info)
        .with_type_info(types);
    lowerer.lower(&hir)
}
