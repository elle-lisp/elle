// audited: 2026-09-20
//! The core.lisp bootstrap: compile and run the core module before any
//! compile context exists, and hand its exports to the one being built.
//!
//! src/pipeline/AGENTS.md
//!
//! core.lisp is the first Elle this process runs, so it compiles through a
//! bare expander with no prelude macros and an analyzer seeded with the
//! primitives alone. Its exports reach user code as globals and macro bodies
//! through the expander core env.

use crate::hir::typeinfer::{DispatchWrapperRegistry, FnInlineRegistry};
use crate::primitives::def::PrimitiveMeta;
use crate::signals::Signal;
use crate::symbol::SymbolTable;
use crate::syntax::Expander;
use crate::value::arena::RootRef;
use crate::vm::VM;

/// core.lisp source, embedded at compile time.
const CORE: &str = include_str!("../core.lisp");

/// Compile and execute core.lisp, storing exports in the Expander's core_env.
///
/// Runs the full pipeline (read → expand → analyze → lower → emit → execute)
/// without using a `CompileCtx` (we're inside its construction). The bare
/// expander has no prelude macros — core.lisp uses only special forms and
/// %-prefixed intrinsics.
pub(super) fn compile_core(
    vm: &mut VM,
    symbols: &mut SymbolTable,
    meta: &mut PrimitiveMeta,
    expander: &mut Expander,
) {
    use crate::hir::{Analyzer, BindingArena, FileForm};
    use crate::lir::{Emitter, Lowerer};
    use crate::reader::read_syntax_all;
    use crate::syntax::Span;
    use std::rc::Rc;

    // core.lisp is one compilation unit with its own working arena, freed
    // when this returns; its exports are `Value`s, which carry no syntax.
    let heap_ptr = vm.heap_ptr;
    let syntax_arena = crate::syntax::SyntaxArena::mint(unsafe { &mut *heap_ptr });
    let syntaxes =
        read_syntax_all(syntax_arena, CORE, "<core>").expect("core.lisp parsing must succeed");

    // Expand with bare expander (no prelude)
    let mut bare_expander = unsafe { Expander::new(heap_ptr) };
    bare_expander.set_arena(syntax_arena);
    let expanded_forms: Vec<_> = syntaxes
        .into_iter()
        .map(|s| bare_expander.expand(s, symbols, vm))
        .collect::<Result<_, _>>()
        .expect("core.lisp expansion must succeed");

    let forms: Vec<FileForm> = expanded_forms
        .iter()
        .map(crate::hir::classify_form)
        .collect();
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
    analyzer.bind_primitives(meta);
    let mut hir = analyzer
        .analyze_file_letrec(forms, span)
        .expect("core.lisp analysis must succeed");
    let prim_values = analyzer.primitive_values().clone();
    let errors = analyzer.take_errors();
    drop(analyzer);

    if !errors.is_empty() {
        for e in &errors {
            eprintln!("core.lisp analysis error: {:?}", e);
        }
        panic!("core.lisp analysis produced {} error(s)", errors.len());
    }

    // core.lisp runs before the instance `CompileCtx` (and its registries) exists,
    // during `on_vm` construction, so throwaway registries are correct here: it
    // defines no container-dispatch wrappers (its `concat`/`reverse` fan to helpers,
    // not single monomorphic-op arms), and its cross-unit-inlineable fns are not
    // recorded for later units. The cross-unit templates every later unit reads
    // (`inc`/`dec`) live in `stdlib.lisp`, which compiles through the instance
    // registries.
    let types = crate::hir::regularize(
        &mut hir,
        &mut arena,
        symbols,
        &mut DispatchWrapperRegistry::default(),
        &mut FnInlineRegistry::default(),
    )
    .expect("core.lisp uses no monomorphic container ops, so the proof obligation holds");

    let pc = crate::lir::intrinsics::PrimitiveClassification::new(meta);
    let region_info =
        crate::hir::analyze_regions_with(&hir, &arena, pc.call_classification.clone());
    if crate::config::get().trace_bits() & crate::config::trace_bits::REGIONS != 0 {
        eprintln!(
            "[trace:regions] cache (core.lisp):\n{}",
            crate::hir::format_regions(&region_info, &arena, Some(symbols))
        );
    }
    let mut lowerer = Lowerer::new(&arena)
        .with_symbols(symbols)
        .with_primitive_classification(pc)
        .with_primitive_values(prim_values)
        .with_region_info(region_info)
        .with_type_info(types);
    let lir_module = lowerer
        .lower(&hir)
        .expect("core.lisp lowering must succeed");

    let mut emitter = Emitter::new();
    let (bytecode, _yield_points, _call_sites) = emitter.emit_module(&lir_module);

    let closure_val = vm
        .execute(&bytecode)
        .expect("core.lisp execution must succeed");

    let closure = closure_val
        .as_closure()
        .expect("core.lisp must return a closure");
    let env = Rc::new(crate::primitives::module_init::build_closure_call_env(
        closure,
        &[],
    ));
    let exports_val = vm
        .execute_code(closure.template.code(), Some(&env))
        .expect("core.lisp export closure must succeed");

    // Root the core export aggregate, not each entry. `exports_val` is the
    // struct returned by core.lisp; it references every core export (each was
    // incref'd into the struct when built), and the per-name `Value`s copied
    // into `core_env`/`meta` below are aliases into those same regions. Under the
    // mint-at-return convention this struct survives on the top-level return
    // mint's +1, which the caller balances at the result's decref_point — so
    // without a root the struct would be freed there, cascade-freeing the exports
    // while `core_env` and `meta` still alias them (dangling reads on later
    // compiles). Registering the struct (and the module closure that produced it)
    // as process roots keeps the exports live for the process and lets the
    // teardown sweep reclaim them by RC cascade. One registration each — these
    // are distinct regions, so no double-decref (R9).
    let heap = unsafe { &mut *vm.heap_ptr };
    crate::value::arena::register_process_root(heap, closure_val, RootRef::Take);
    crate::value::arena::register_process_root(heap, exports_val, RootRef::Take);

    let exports_struct = exports_val
        .as_struct()
        .expect("core.lisp must return a struct");
    for (key, value) in exports_struct.iter() {
        if let crate::value::types::TableKey::Keyword(hash) = key {
            // The export struct was read from module source, so its key
            // spellings are in the memo; a miss is a missed learning site.
            let name = symbols
                .keyword_name(*hash)
                .map(str::to_string)
                .unwrap_or_else(|| panic!("module export key {:#x} has no learned spelling", hash));
            // core_env: name-keyed, used by eval_syntax for macro bodies
            expander.core_env.insert(name.clone(), *value);
            // meta: SymbolId-keyed, used by compile_file for user code
            let sym_id = symbols.intern(&name);
            let signal = if let Some(c) = value.as_closure() {
                c.template.signal()
            } else {
                Signal::silent()
            };
            meta.signals.insert(sym_id, signal);
            meta.functions.insert(sym_id, *value);
        }
    }

    // core.lisp's tree has done its work: nothing beyond this point reads it,
    // and the macros it might have defined were copied to the template arena
    // as they were registered.
    unsafe { (*heap_ptr).decref_region_if_present(syntax_arena.region()) };
}
