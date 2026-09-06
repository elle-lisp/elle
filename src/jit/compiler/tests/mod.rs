// audited: 2026-09-05
// docs/impl/jit.md
//! What the solo-compilation gate accepts and rejects, and what the compiled
//! entry it produces records about itself.

use super::*;
use crate::lir::testkit::LirFixture;
use crate::lir::{BinOp, LirInstr, Reg, Terminator};
use crate::signals::Signal;
use crate::value::Arity;

mod batch;
mod blueprint;
mod clif;
mod regions;

fn make_simple_lir() -> LirFunction {
    // Create a simple function that returns its first argument
    // fn(x) -> x
    // The LIR uses LoadCapture to access parameters.
    // With num_captures=0, LoadCapture index 0 loads from args[0].
    LirFixture::new(Arity::Exact(1))
        .signal(Signal::silent())
        .block(
            0,
            vec![LirInstr::LoadCapture {
                dst: Reg(0),
                index: 0,
            }],
            Terminator::Return(Reg(0)),
        )
        .build()
}

fn make_add_lir() -> LirFunction {
    // Create a function that adds two arguments
    // fn(x, y) -> x + y
    // With num_captures=0, LoadCapture index 0 and 1 load from args[0] and args[1].
    LirFixture::new(Arity::Exact(2))
        .signal(Signal::silent())
        .block(
            0,
            vec![
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::LoadCapture {
                    dst: Reg(1),
                    index: 1,
                },
                LirInstr::BinOp {
                    dst: Reg(2),
                    op: BinOp::Add,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
}

#[test]
fn test_compile_identity() {
    let lir = make_simple_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, Vec::new())
        .expect("Failed to compile");

    // Call the compiled function with self_tag=0, self_payload=0 (no self-tail-call).
    // A real VM is required: every compiled function's prologue pushes an
    // activation region-map frame (`elle_jit_push_region_map`).
    let mut vm = crate::vm::VM::new();
    let args = [crate::value::Value::int(42)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            1,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    assert_eq!(value.as_int(), Some(42));
}

#[test]
fn test_compile_add() {
    let lir = make_add_lir();
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, Vec::new())
        .expect("Failed to compile");

    // Call the compiled function with self_tag=0, self_payload=0
    let mut vm = crate::vm::VM::new();
    let args = [crate::value::Value::int(10), crate::value::Value::int(32)];
    let value = unsafe {
        code.call(
            std::ptr::null(),
            args.as_ptr(),
            2,
            &mut vm as *mut crate::vm::VM as *mut (),
            0,
            0,
        )
    }
    .to_value();
    assert_eq!(value.as_int(), Some(42));
}

#[test]
fn test_accept_polymorphic() {
    let mut lir = make_simple_lir();
    lir.signal = Signal::polymorphic(0);

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, Vec::new());
    assert!(
        result.is_ok(),
        "JIT should accept polymorphic functions (runtime dispatch handles callables): {:?}",
        result,
    );
}

#[test]
fn test_accept_yielding() {
    let mut lir = make_simple_lir();
    lir.signal = Signal::yields();

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    // Should compile (no Yield terminators in this simple LIR)
    let result = compiler.compile(&lir, None, Vec::new());
    assert!(result.is_ok());
}

#[test]
fn test_compile_yielding_function() {
    use crate::lir::YieldPointInfo;

    let func = LirFixture::new(Arity::Exact(0))
        .signal(Signal::yields())
        .yield_points(vec![YieldPointInfo {
            resume_ip: 5,
            stack_regs: vec![],
            num_locals: 0,
        }])
        .block(
            0,
            vec![LirInstr::Const {
                dst: Reg(0),
                value: crate::lir::LirConst::Int(42),
            }],
            Terminator::Emit {
                signal: crate::value::fiber::SIG_YIELD,
                value: Reg(0),
                resume_label: Label(1),
            },
        )
        .block(
            1,
            vec![LirInstr::LoadResumeValue { dst: Reg(1) }],
            Terminator::Return(Reg(1)),
        )
        .build();

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&func, None, Vec::new());
    assert!(
        result.is_ok(),
        "Yielding function should compile: {:?}",
        result.err()
    );
    assert_eq!(result.unwrap().yield_points.len(), 1);
}

#[test]
fn test_reject_struct_variadic() {
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::Struct;

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, Vec::new());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "Struct variadic functions should be rejected: {:?}",
        result,
    );
}

#[test]
fn test_reject_strict_struct_variadic() {
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::StrictStruct(vec!["key".to_string()]);

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, Vec::new());
    assert!(
        matches!(result, Err(JitError::UnsupportedInstruction(_))),
        "StrictStruct variadic functions should be rejected: {:?}",
        result,
    );
}

#[test]
fn test_compile_list_variadic() {
    // AtLeast(1) + VarargKind::List should now compile successfully.
    // fn(x & rest) -> x  (ignores rest, just returns first arg)
    let mut lir = make_simple_lir();
    lir.arity = Arity::AtLeast(1);
    lir.vararg_kind = crate::hir::VarargKind::List;
    lir.num_params = 2; // x + rest

    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let result = compiler.compile(&lir, None, Vec::new());
    assert!(
        result.is_ok(),
        "List variadic functions should compile: {:?}",
        result.err(),
    );
}

#[test]
fn compile_records_entry_in_code_address_registry() {
    let mut lir = make_simple_lir();
    lir.name = Some("registry-probe-solo".to_string());
    let compiler = JitCompiler::new().expect("Failed to create compiler");
    let code = compiler
        .compile(&lir, None, Vec::new())
        .expect("Failed to compile");
    let entry = code.fn_ptr() as usize;
    let name = crate::jit::registry::snapshot()
        .into_iter()
        .find(|(addr, _)| *addr == entry)
        .map(|(_, name)| name);
    assert_eq!(name.as_deref(), Some("registry-probe-solo"));
}
