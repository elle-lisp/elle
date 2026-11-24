// audited: 2026-11-24
// docs/impl/verifier.md
//! Whether a compiler runs Cranelift's verifier: its setting decides, and a
//! verifying compiler refuses IR that is not well formed.

use super::*;
use cranelift_codegen::ir::types::I32;

/// The counter-factual: Cranelift verifies by default, so a constructor that
/// ignored its argument would verify every compile, and a user build would
/// pay for a check only a test needs.
#[test]
fn a_compiler_built_with_the_verifier_off_does_not_verify() {
    let compiler = JitCompiler::new(JitVerifier::Off).expect("Failed to create compiler");
    assert!(!compiler.verifies());
}

#[test]
fn a_compiler_built_with_the_verifier_on_verifies() {
    assert!(verified().verifies());
}

/// Define `fn() -> i64` whose body returns an `i32`, which no verifier admits,
/// and answer what defining it says.
fn define_ill_typed(compiler: &mut JitCompiler) -> Result<(), String> {
    let mut sig = compiler.module.make_signature();
    sig.call_conv = CallConv::SystemV;
    sig.returns.push(AbiParam::new(I64));
    let func_id = compiler
        .module
        .declare_function("ill_typed_probe", Linkage::Local, &sig)
        .expect("declare");
    let mut ctx = compiler.module.make_context();
    ctx.func.signature = sig;
    ctx.func.name = UserFuncName::user(0, func_id.as_u32());

    let mut builder_ctx = FunctionBuilderContext::new();
    let mut builder = FunctionBuilder::new(&mut ctx.func, &mut builder_ctx);
    let entry = builder.create_block();
    builder.switch_to_block(entry);
    builder.seal_block(entry);
    let narrow = builder.ins().iconst(I32, 7);
    builder.ins().return_(&[narrow]);
    finalize_function(builder, &compiler.module);

    compiler
        .module
        .define_function(func_id, &mut ctx)
        .map(|_| ())
        .map_err(|e| format!("{e:?}"))
}

/// The trap: the refusal is the verifier's only when the verifier ran, so this
/// test builds its compiler with the setting on rather than trusting the
/// default.
#[test]
fn a_verifying_compiler_refuses_ill_typed_ir() {
    let err = define_ill_typed(&mut verified())
        .expect_err("a verifying compiler defined a function that returns the wrong type");
    assert!(
        err.contains("Verifier"),
        "the refusal must be the verifier's, got: {err}"
    );
}

/// A tail call that strands two merged arenas hands their slots to its helper
/// in a stack slot. The trap: `stack_store`'s first operand is the address
/// type, not the stored value's, and a narrow one gives a 32-bit address the
/// verifier refuses. A refused function runs uncompiled, so only a test of the
/// compile itself sees the refusal.
#[test]
fn a_tail_call_that_strands_two_arenas_verifies() {
    let lir = LirFixture::new(Arity::Exact(1))
        .block(
            0,
            &[
                InstrRef::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                InstrRef::TailCall {
                    dst: Reg(1),
                    func: Reg(0),
                    args: &[],
                    arity_checked: false,
                    region: crate::hir::region::StaticRegion::new(9).expect("nonzero"),
                    defer_callee_release: false,
                    deferred_release_slots: crate::lir::ArenaSlots::new(&[7, 8]),
                    borrowed_arg_slots: crate::lir::Slots::new(&[]),
                    late_mints: crate::lir::Offers::NONE,
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .build();
    let result = verified().compile(&lir.view(), crate::jit::ChildTable::EMPTY);
    assert!(
        result.is_ok(),
        "a tail call's arena slots must compile under the verifier: {:?}",
        result.err()
    );
}
