// audited: 2026-11-18
// docs/impl/jit-closure.md
//! A function that builds a closure compiles: its `MakeClosure` becomes a call
//! to the helper, over the header its child table holds.

use super::clif::{called_refs, func_refs};
use super::*;
use crate::hir::region::StaticRegion;
use crate::jit::ChildTable;
use crate::lir::{ClosureId, ConstRef, Emitter, FrozenModule};
use crate::value::{CodeArena, Value};

/// A nullary function whose whole body is one `MakeClosure` of closure 0.
fn outer_lir() -> LirOwned {
    LirFixture::new(Arity::Exact(0))
        .signal(Signal::silent())
        .block(
            0,
            &[InstrRef::MakeClosure {
                dst: Reg(0),
                closure_id: ClosureId(0),
                captures: &[],
                region: StaticRegion::new(2).unwrap(),
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

/// `outer_lir`, and the child table the emitter writes for it: one header,
/// over a lambda that answers nil.
fn outer_and_children() -> (LirOwned, Vec<Value>) {
    let lambda = LirFixture::new(Arity::Exact(0))
        .block(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Nil,
            }],
            Terminator::Return(Reg(0)),
        )
        .build();
    let module = FrozenModule {
        entry: outer_lir(),
        closures: vec![lambda],
    };
    let code = CodeArena::mint(unsafe { &mut *crate::value::arena::leaked_test_heap() });
    let children = Emitter::new(code).emit_module(&module).bytecode.children;
    (module.entry, children)
}

/// The module function id `compiler` declared `name` under.
fn helper_id(compiler: &JitCompiler, name: &str) -> u32 {
    use cranelift_module::{FuncOrDataId, Module};
    match compiler.module.get_name(name) {
        Some(FuncOrDataId::Func(id)) => id.as_u32(),
        other => panic!("the JIT declares no helper {name}: {other:?}"),
    }
}

/// The translator lowers `MakeClosure` to a call of `elle_jit_make_closure`.
/// The rendered IR is the whole claim: running the code needs the parent's
/// payload, which a fixture has none of.
///
/// The counter-factual is a translator that refuses the instruction, as it
/// did when the JIT had no lowering for it: it renders nothing.
#[test]
fn a_make_closure_calls_the_closure_helper() {
    let (outer, children) = outer_and_children();
    let compiler = verified();
    let make_closure = helper_id(&compiler, "elle_jit_make_closure");
    let clif = compiler
        .clif_text(&outer.view(), ChildTable::new(&children))
        .expect("a function that builds a closure translates");
    let refs = func_refs(&clif);
    let called: Vec<u32> = called_refs(&clif)
        .iter()
        .filter_map(|name| refs.get(name).copied())
        .collect();
    assert!(
        called.contains(&make_closure),
        "a MakeClosure must reach `elle_jit_make_closure` (u0:{make_closure}); got \
         {called:?} in:\n{}",
        clif.join("\n")
    );
}

/// `compile` admits a function that builds a closure.
#[test]
fn compile_admits_a_function_that_builds_a_closure() {
    let (outer, children) = outer_and_children();
    let result = verified().compile(&outer.view(), ChildTable::new(&children));
    assert!(
        result.is_ok(),
        "a function holding a MakeClosure must compile; got {:?}",
        result.err()
    );
}

/// A `MakeClosure` whose header the child table does not hold is invalid LIR:
/// the compiled code would have no header to build over.
///
/// The counter-factual is a translator that reads past the table, and bakes
/// whatever words lie there into the call.
#[test]
fn a_make_closure_with_no_header_is_invalid_lir() {
    let result = verified().compile(&outer_lir().view(), ChildTable::EMPTY);
    assert!(
        matches!(result, Err(JitError::InvalidLir(_))),
        "a MakeClosure with no child-table entry must be invalid LIR; got {:?}",
        result.err()
    );
}
