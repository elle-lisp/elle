// audited: 2026-09-05
// docs/impl/region/template.md
//! The blueprint the JIT builds for a nested lambda at a `MakeClosure`.

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::{ClosureId, LirConst};

/// A nullary lambda carrying one of everything the blueprint has to copy off
/// its `LirFunction`: both release tables, the region table, and a merge set.
fn nested_lambda_lir() -> LirFunction {
    let mut func = LirFixture::new(Arity::Exact(0))
        .name("nested")
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: LirConst::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();
    func.region_table = vec![StaticRegion::new(2).unwrap(), StaticRegion::new(5).unwrap()];
    func.merged_slots = vec![StaticRegion::new(5).unwrap()];
    func.frame_release_slots = vec![3, 7];
    func.frame_release_regions = vec![
        StaticRegion::new(11).unwrap(),
        StaticRegion::new(13).unwrap(),
    ];
    func
}

/// A nullary function whose whole body is one `MakeClosure` of closure 0.
fn outer_lir() -> LirFunction {
    LirFixture::new(Arity::Exact(0))
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: vec![],
                region: StaticRegion::new(2).unwrap(),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// The blueprints translating `outer` builds, with `nested` as the module's
/// only closure.
///
/// Translation is driven directly rather than through `JitCompiler::compile`,
/// which rejects a function holding a `MakeClosure` before the translator sees
/// it. The blueprint is built all the same, and it is what a closure the
/// compiled code materializes reads its code object from.
fn closure_protos(
    outer: &LirFunction,
    nested: LirFunction,
) -> Vec<std::rc::Rc<crate::value::TemplateProto>> {
    let mut compiler = JitCompiler::new().expect("Failed to create compiler");
    let sig = compiler.make_jit_signature();
    let func_id = compiler
        .module
        .declare_function("outer", Linkage::Local, &sig)
        .expect("Failed to declare");
    let mut ctx = compiler.module.make_context();
    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, func_id.as_u32());
    let (protos, _) = compiler
        .translate_function(outer, &mut ctx.func, None, None, vec![nested])
        .expect("Failed to translate");
    protos
}

#[test]
fn a_nested_lambdas_jit_blueprint_carries_the_frame_release_tables() {
    // Counter-factual: leaving both tables to the empty value
    // `TemplateProto::new` supplies fails nothing that runs. The closure built
    // from such a blueprint carries real bytecode and returns the right
    // answers; what it loses is one error exit's walk, which strands every
    // region the abandoned frame still owed.
    let protos = closure_protos(&outer_lir(), nested_lambda_lir());
    assert_eq!(protos.len(), 1, "one MakeClosure builds one blueprint");
    assert_eq!(
        protos[0].frame_release_slots,
        vec![3u16, 7],
        "the value route's slots reach the blueprint",
    );
    assert_eq!(
        protos[0].frame_release_regions,
        vec![11u32, 13],
        "the slot route's regions reach the blueprint",
    );
}

#[test]
fn a_nested_lambdas_jit_blueprint_carries_the_regions_its_body_names() {
    // The two region tables travel beside the release tables and off the same
    // `LirFunction`, so one omission is as silent as the other.
    let protos = closure_protos(&outer_lir(), nested_lambda_lir());
    assert_eq!(
        protos[0].region_table,
        vec![StaticRegion::new(2).unwrap(), StaticRegion::new(5).unwrap()],
        "the slots the body mints reach the blueprint",
    );
    let merged: Vec<u32> = {
        let mut m: Vec<u32> = protos[0].merged_slots.iter().copied().collect();
        m.sort_unstable();
        m
    };
    assert_eq!(merged, vec![5u32], "the merge set reaches the blueprint");
    assert_eq!(protos[0].name.as_deref(), Some("nested"));
    assert_eq!(protos[0].arity, Arity::Exact(0));
}
