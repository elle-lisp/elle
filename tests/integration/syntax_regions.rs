// Where a syntax tree lives, and how long (docs/impl/syntax.md § "Where a node
// lives"). The tree is region data now, so its lifetime is a region's lifetime.
// Two claims the rest of the compiler leans on: a unit's tree dies with the
// unit, and a macro template outlives the unit that defined it.

use elle::pipeline::compile_file_repl;
use elle::runtime::Runtime;
use elle::syntax::{Syntax, SyntaxHeap};
use elle::{compile_file, eval_all};

/// How many regions the instance's heap has live.
fn live_regions(rt: &mut Runtime) -> usize {
    rt.heap().region_info_vec().len()
}

/// Compiling the same unit repeatedly must not grow the heap's live-region
/// count: the working arena the unit's tree was born in is freed when its
/// bytecode is built.
///
/// The counter-factual: a working arena that leaked would add one region — and
/// the unit's whole tree — per compile, so a REPL or a server compiling in a
/// loop would grow without bound.
#[test]
fn a_units_syntax_arena_is_freed_with_the_unit() {
    let mut rt = Runtime::new();
    let source = "(defn double (x) (* x 2)) (double 21)";

    // Warm up: the first compiles fill lazily-populated instance state
    // (transformer caches, cross-unit registries) that is not this unit's tree.
    for _ in 0..2 {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file(source, symbols, cctx, "<warm>").expect("compiles");
    }

    let before = live_regions(&mut rt);
    for _ in 0..8 {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file(source, symbols, cctx, "<unit>").expect("compiles");
    }
    let after = live_regions(&mut rt);

    assert!(
        after <= before,
        "eight compiles left {} extra live regions; a unit's syntax arena must \
         be released when the unit finishes",
        after as i64 - before as i64
    );
}

/// A macro defined in one unit expands correctly in a later one. Its template
/// was copied into the instance's template arena as it was registered, so it
/// survives the death of the working arena the `defmacro` form was read into.
///
/// The REPL path is the one that persists a macro across units
/// (`compile_file_repl` merges the unit's macros back into the instance), so
/// it is what this drives.
///
/// The counter-factual: a template left in the defining unit's arena would be
/// freed with that unit, and this expansion would read released pages.
#[test]
fn a_macro_template_outlives_the_unit_that_defined_it() {
    let mut rt = Runtime::new();

    {
        let (_vm, symbols, cctx) = rt.parts();
        let (_bytecode, expander) = compile_file_repl(
            "(defmacro triple (x) `(* ,x 3))",
            symbols,
            cctx,
            "<definer>",
        )
        .expect("the defmacro unit compiles");
        cctx.register_repl_macros(expander.macros());
    }
    // More units, so the definer's arena is long gone and its region id is
    // free to be recycled under the later compiles.
    for i in 0..4 {
        let (_vm, symbols, cctx) = rt.parts();
        compile_file("(+ 1 2)", symbols, cctx, &format!("<filler{i}>")).expect("compiles");
    }

    let (vm, symbols, cctx) = rt.parts();
    let out = eval_all("(triple 14)", symbols, vm, cctx, "<user>").expect("expands and runs");
    assert_eq!(out.as_int(), Some(42));
}

/// Reading into a `SyntaxHeap` and letting it drop reclaims the tree — the
/// standalone form `elle fmt`, the pre-VM `unicode!` scan, and the epoch
/// rewriter use, none of which has a runtime in reach.
#[test]
fn a_standalone_syntax_heap_reads_and_reclaims_without_a_runtime() {
    let mut home = SyntaxHeap::new();
    let arena = home.arena();
    let form: Syntax = elle::reader::read_syntax(arena, "(a (b c) d)", "<scratch>").expect("parses");
    let items = form.as_list().expect("a list");
    assert_eq!(items.len(), 3);
    assert_eq!(items[0].as_symbol(), Some("a"));
    assert_eq!(items[1].as_list().map(<[Syntax]>::len), Some(2));
    // Dropping `home` releases the region; nothing above outlives this scope.
    drop(home);
}
