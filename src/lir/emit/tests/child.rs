// audited: 2026-11-18
//! Which child-table entry `LirView::child_of` names, against the table the emitter wrote.
//!
//! docs/impl/jit-closure.md

use crate::compiler::stdlib_cache::StdlibCache;
use crate::lir::{ClosureId, InstrRef, LirView};
use crate::value::{ClosureTemplate, Value};

/// The `ClosureId` of each `MakeClosure` in `lir`, in the order the emitter
/// meets them.
fn make_closure_ids(lir: &LirView<'_>) -> Vec<ClosureId> {
    lir.nodes()
        .filter_map(|n| match n.instr() {
            InstrRef::MakeClosure { closure_id, .. } => Some(closure_id),
            _ => None,
        })
        .collect()
}

/// Every code object `t`'s child tables reach, `t` included, each once.
fn reachable(t: ClosureTemplate, seen: &mut Vec<*const ()>, out: &mut Vec<ClosureTemplate>) {
    if seen.contains(&t.payload_backing()) {
        return;
    }
    seen.push(t.payload_backing());
    for i in 0..t.num_children() {
        reachable(t.child(i), seen, out);
    }
    out.push(t);
}

/// Each header as the two words compiled code bakes in.
fn words(v: Value) -> (u64, u64) {
    (v.tag, v.payload)
}

/// For every `MakeClosure` the standard library compiles to, the entry
/// `child_of` names holds the header that the instruction's own entry holds.
/// The interpreter reads the entry at the instruction's ordinal, and compiled
/// code reads the one `child_of` answers, so both tiers build over one code
/// object.
///
/// The counter-factual is an index counted over another walk: one that skips
/// the nodes past a terminating instruction, or one that counts each
/// `ClosureId` once. Each reads a sibling's header at a later `MakeClosure`,
/// and the closure it builds runs the wrong body.
#[test]
fn child_of_names_the_header_each_make_closure_builds_over() {
    let mut rt = crate::runtime::Runtime::with_stdlib_cache(StdlibCache::Off);
    let (_, symbols, cctx) = rt.parts();
    let unit = crate::pipeline::compile_file(
        crate::pipeline::sources::STDLIB,
        symbols,
        cctx,
        "stdlib.lisp",
    )
    .expect("the standard library compiles");

    let mut templates = Vec::new();
    reachable(unit.entry().clone(), &mut Vec::new(), &mut templates);
    let mut checked = 0;
    for t in &templates {
        let Some(lir) = t.lir() else { continue };
        let children = t.payload().children();
        let ids = make_closure_ids(&lir);
        assert_eq!(
            ids.len(),
            children.len(),
            "{}: one child-table entry per MakeClosure",
            t.display_label()
        );
        for (k, id) in ids.into_iter().enumerate() {
            let at = lir
                .child_of(id)
                .unwrap_or_else(|| panic!("{}: no entry for {id:?}", t.display_label()));
            assert_eq!(
                words(children[at.get()]),
                words(children[k]),
                "{}: MakeClosure {k} of {id:?} reads entry {} for a different header",
                t.display_label(),
                at.get()
            );
            checked += 1;
        }
    }
    assert!(
        checked > 100,
        "the standard library builds {checked} closures, so the walk missed most"
    );
}

/// A function that builds one lambda at two sites, with another between them,
/// gets one child-table entry per site and one header per lambda, and
/// `child_of` names an entry holding that lambda's header. No standard-library
/// function builds one lambda twice, so the test above cannot see this shape.
///
/// The counter-factual is an index that counts each `ClosureId` once: it names
/// entry 1 for closure 1, which holds closure 0's header from its second site.
#[test]
fn child_of_names_the_lambdas_header_where_two_sites_build_it() {
    use crate::hir::region::StaticRegion;
    use crate::lir::testkit::LirFixture;
    use crate::lir::{Emitter, FrozenModule, Reg, Terminator};
    use crate::value::{Arity, CodeArena};

    let make = |dst: u32, id: u32| InstrRef::MakeClosure {
        dst: Reg(dst),
        closure_id: ClosureId(id),
        captures: &[],
        region: StaticRegion::new(2).unwrap(),
    };
    let entry = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[make(0, 0), make(1, 0), make(2, 1)],
            Terminator::Return(Reg(2)),
        )
        .build();
    let lambda = || {
        LirFixture::new(Arity::Exact(0))
            .block(
                0,
                &[InstrRef::Const {
                    dst: Reg(0),
                    value: crate::lir::ConstRef::Nil,
                }],
                Terminator::Return(Reg(0)),
            )
            .build()
    };
    let module = FrozenModule {
        entry,
        closures: vec![lambda(), lambda()],
    };
    let code = CodeArena::mint(unsafe { &mut *crate::value::arena::leaked_test_heap() });
    let children = Emitter::new(code).emit_module(&module).bytecode.children;

    let view = module.entry.view();
    assert_eq!(children.len(), 3, "one entry per MakeClosure");
    assert_ne!(
        words(children[0]),
        words(children[2]),
        "two lambdas share a header, so this test tells nothing apart"
    );
    for (k, id) in [(0, 0), (1, 0), (2, 1)] {
        let at = view.child_of(ClosureId(id)).expect("an entry");
        assert_eq!(
            words(children[at.get()]),
            words(children[k]),
            "MakeClosure {k} of closure {id} reads entry {}",
            at.get()
        );
    }
}
