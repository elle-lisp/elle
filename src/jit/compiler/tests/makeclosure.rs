// audited: 2026-10-06
// docs/impl/jit.md
//! The JIT has no translation for `MakeClosure`: the translator refuses it as it
//! refuses any instruction it cannot lower.

use super::*;
use crate::hir::region::StaticRegion;
use crate::lir::ClosureId;

/// A nullary function whose whole body is one `MakeClosure` of closure 0.
fn outer_lir() -> LirOwned {
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

/// The translator itself refuses `MakeClosure`, not only the admission check in
/// front of `compile`. The rendered-IR path skips that check, so it is the one
/// that reaches the translator.
///
/// The counter-factual is a translator that still lowers the instruction: it
/// has to build the nested lambda's code object on the worker's thread, where
/// there is no heap to write a payload into, and it answers a lookup failure
/// for the closure it was never handed instead of a refusal.
#[test]
fn the_translator_refuses_make_closure() {
    let result = JitCompiler::new()
        .expect("Failed to create compiler")
        .clif_text(&outer_lir().view());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "a MakeClosure must be refused as unsupported; got {result:?}",
    );
}

/// `compile` refuses the same function, before translation.
#[test]
fn compile_refuses_make_closure() {
    let result = JitCompiler::new()
        .expect("Failed to create compiler")
        .compile(&outer_lir().view());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "a function holding a MakeClosure must not compile; got {:?}",
        result.err()
    );
}
