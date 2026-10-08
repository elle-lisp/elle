// audited: 2026-10-07
//! When a macro's compiled transformer is freed: with the last definition that holds it, on the heap it was compiled on.
//!
//! src/syntax/expand/AGENTS.md

use super::ownership::{mid_run_discriminator, mid_run_growth};
use super::*;
use crate::pipeline::{compile_file, eval_all, CompileCtx};
use crate::symbol::SymbolTable;
use crate::vm::VM;

/// An eval that defines `m` and expands it once: its compile fills a fresh
/// transformer cell, and the definition before it drops.
const REDEFINE_AND_USE: &str = "(eval '(begin (defmacro m [x] x) (m 1)))";

/// The control: the same definition, never expanded, so no transformer is
/// ever compiled. Whatever else an eval costs, this costs too.
const REDEFINE_ONLY: &str = "(eval '(begin (defmacro m [x] x) 1))";

/// The growth of `gauge` over 200 runs of `body`, on a runtime with no stdlib.
fn growth(body: &str, gauge: &str) -> i64 {
    mid_run_growth(Runtime::without_stdlib(), "", body, gauge)
}

// The VM's `eval` expander lives as long as the VM, so a redefinition through
// `eval` is the one way its old definition goes. Each run of the subject
// replaces a definition whose transformer it compiled on the run before.
//
// The counter-factual is a cell that never releases: every replaced definition
// strands its transformer's region, and the subject grows by at least one
// region and one object per run over the control.
#[test]
fn a_redefined_macro_frees_the_transformer_it_replaces() {
    for gauge in ["arena/region-count", "arena/count"] {
        let live = mid_run_discriminator(Runtime::without_stdlib(), gauge);
        assert!(
            live >= 150,
            "{gauge} reads {live} for a shape that retains every run"
        );
        let subject = growth(REDEFINE_AND_USE, gauge);
        let control = growth(REDEFINE_ONLY, gauge);
        assert!(
            subject - control < 20,
            "{gauge}: 200 redefinitions grew {subject} with each transformer used and {control} \
             with none compiled"
        );
    }
}

/// The growth of the instance heap's regions and objects over `n` compiles of
/// `src`, after `warm` compiles that fill whatever a first compile fills.
fn compile_growth(src: &str, warm: usize, n: usize) -> (i64, i64) {
    let mut rt = Runtime::without_stdlib();
    let compile = |rt: &mut Runtime| {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file(src, symbols, cctx, "<macros>").expect("compiles");
    };
    for _ in 0..warm {
        compile(&mut rt);
    }
    let regions = rt.heap().active_region_count() as i64;
    let objects = rt.heap().visible_len() as i64;
    for _ in 0..n {
        compile(&mut rt);
    }
    (
        rt.heap().active_region_count() as i64 - regions,
        rt.heap().visible_len() as i64 - objects,
    )
}

// A macro a unit defines lives in the unit's clone of the instance expander
// alone, so its definition drops when the compile returns.
//
// The counter-factual is the same cell that never releases: each compile
// strands the transformer it compiled for the unit's own use of the macro.
#[test]
fn a_macro_a_unit_defines_is_freed_with_the_unit() {
    let (regions, objects) = compile_growth("(defmacro m [x] x) (m 1)", 5, 100);
    let (control_regions, control_objects) = compile_growth("(defmacro m [x] x) 1", 5, 100);
    assert!(
        regions - control_regions < 10,
        "100 compiles that each expand their own macro grew {regions} regions, \
         against {control_regions} for a macro never expanded"
    );
    assert!(
        objects - control_objects < 10,
        "100 compiles that each expand their own macro grew {objects} objects, \
         against {control_objects} for a macro never expanded"
    );
}

/// The regions a runtime with no stdlib leaves after running `src` and
/// tearing down.
fn residue_after(src: &str) -> usize {
    let mut rt = Runtime::without_stdlib();
    {
        let (vm, symbols, cctx) = rt.parts();
        eval_all(src, symbols, vm, cctx, "<macros>").expect("runs");
    }
    rt.teardown().live_regions
}

// The `eval` expander still holds `m` at teardown, with its transformer
// compiled. Teardown must free it before it counts what is left.
//
// The counter-factual is a teardown that releases the compile context's cells
// alone: the `eval` expander drops with the VM, after the count, and the
// subject leaves its transformer's region behind where the control leaves
// none.
#[test]
fn teardown_frees_the_transformers_the_eval_expander_holds() {
    assert_eq!(
        residue_after(REDEFINE_AND_USE),
        residue_after(REDEFINE_ONLY),
        "a macro defined and expanded through eval leaves more behind at teardown \
         than one defined and never expanded"
    );
}

// A standalone compile context expands `eval` on the caller's VM, so a
// transformer that expansion compiles lives on the caller's heap, while the
// definition belongs to the compile context. The release must reach the heap
// the transformer is on.
//
// The counter-factual is a release that decrefs on the compile context's own
// heap: the caller's heap keeps the region, and the compile heap loses a
// reference to some region of its own that happens to bear the same id.
#[test]
fn a_transformer_is_released_on_the_heap_it_was_compiled_on() {
    let mut vm = VM::new();
    let mut symbols = SymbolTable::new();
    crate::register_primitives(&mut vm, &mut symbols);
    let mut cctx = CompileCtx::new();
    let compile_heap = cctx.heap_ptr();
    assert!(
        cctx.macros()["when"].transformer().get().is_none(),
        "the prelude compiles no transformer, so the eval below compiles `when`'s"
    );

    crate::pipeline::eval(
        "(when true 1)",
        &mut symbols,
        &mut vm,
        &mut cctx,
        "<macros>",
    )
    .expect("evaluates");
    let transformer = cctx.macros()["when"]
        .transformer()
        .get()
        .expect("the eval filled the compile context's cell");
    assert!(
        vm.heap().value_in_region_store(transformer),
        "the transformer lives on the caller's heap"
    );

    let regions = vm.heap().active_region_count();
    let over_frees = unsafe { &*compile_heap }.over_frees();
    drop(cctx);
    assert!(
        vm.heap().active_region_count() < regions,
        "dropping the compile context left the transformer's region on the caller's heap"
    );
    assert_eq!(
        unsafe { &*compile_heap }.over_frees(),
        over_frees,
        "the release reached the compile context's heap"
    );
}

// A transformer that runs `eval` compiles the inner macro's transformer while
// the outer expansion's allocation scope is open, and that scope's reclaim
// balances every reference a scan of the heap cannot explain. A cell's
// reference is held from Rust. The nested `eval` builds an expander of its
// own, which the outer one replaces on the VM, so the inner cell drops after
// the reclaim has run.
//
// The trap is a reclaim that counts the cell's reference as scratch: it frees
// the inner transformer, and the cell's release then decrefs a freed region.
// A debug build stops on that over-free; a release build counts it.
#[test]
fn a_transformer_compiled_inside_another_expansion_is_released_once() {
    let mut rt = Runtime::without_stdlib();
    let over_frees = rt.heap().over_frees();
    {
        let (vm, symbols, cctx) = rt.parts();
        let value = eval_all(
            "(eval '(begin \
               (defmacro outer [] \
                 (begin (eval '(begin (defmacro inner [x] x) (inner 1))) 42)) \
               (outer)))",
            symbols,
            vm,
            cctx,
            "<macros>",
        )
        .expect("runs");
        assert_eq!(value.as_int(), Some(42));
    }
    assert_eq!(rt.heap().over_frees(), over_frees, "a release ran twice");
    assert_eq!(
        rt.teardown().live_regions,
        0,
        "a transformer compiled inside another expansion outlived teardown"
    );
}
