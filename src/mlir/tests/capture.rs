// audited: 2026-09-06
// docs/impl/mlir.md
//! A closure body that reads its captured environment: lowered, executed, and
//! refused by the SPIR-V tier.

use super::*;

// ── Capture tests ──────────────────────────────────────────────

/// Build LIR: fn(x) { return cap[0] + x } with num_captures=1
fn make_capture_add() -> LirFunction {
    // Env layout: [cap0, param0]
    LirFixture::new(Arity::Exact(1))
        .name("capture_add")
        .signal(Signal::errors())
        .num_captures(1)
        .block(
            0,
            vec![
                // Load capture (index 0 = first capture)
                LirInstr::LoadCapture {
                    dst: Reg(0),
                    index: 0,
                },
                // Load param (index 1 = first param, since 1 capture)
                LirInstr::LoadCaptureRaw {
                    dst: Reg(1),
                    index: 1,
                },
                // cap + param
                LirInstr::binop(Reg(2), BinOp::Add, Reg(0), Reg(1)),
            ],
            Terminator::Return(Reg(2)),
        )
        .build()
}

#[test]
fn test_lower_capture_add() {
    let context = lower::create_context();
    let func = make_capture_add();
    // num_captures=1, capture_types=0 (int), param_types=0 (int)
    let result = lower::lower_to_module(&context, &func, 1, 0, 0);
    assert!(result.is_ok(), "capture_add lowering should succeed");
    let (module, _) = result.unwrap();
    let text = module.as_operation().to_string();
    // 2-param MLIR function (1 capture + 1 param)
    assert!(
        text.contains("arith.addi"),
        "should contain arith.addi: {}",
        text
    );
}

#[test]
fn test_execute_capture_add() {
    let context = lower::create_context();
    let func = make_capture_add();
    // num_captures=1, capture_types=0, param_types=0
    let (mut module, _) =
        lower::lower_to_module(&context, &func, 1, 0, 0).expect("lowering should succeed");
    let pm = melior::pass::PassManager::new(&context);
    pm.add_pass(melior::pass::conversion::create_to_llvm());
    pm.run(&mut module).expect("LLVM conversion should succeed");
    let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);
    // Call with capture=5, arg=3 → should return 8
    let mut cap: i64 = 5;
    let mut arg: i64 = 3;
    let mut result: i64 = 0;
    unsafe {
        engine
            .invoke_packed(
                "capture_add",
                &mut [
                    &mut cap as *mut i64 as *mut (),
                    &mut arg as *mut i64 as *mut (),
                    &mut result as *mut i64 as *mut (),
                ],
            )
            .unwrap();
    }
    assert_eq!(result, 8, "capture(5) + arg(3) should be 8");
}

#[test]
fn test_spirv_rejects_captures() {
    let func = make_capture_add();
    let result = lower_to_spirv(&func, 256);
    assert!(result.is_err(), "SPIR-V should reject captures");
    assert!(
        result.unwrap_err().contains("captures not supported"),
        "error should mention captures"
    );
}

// ── Env indices and destination registers are separate spaces ───

/// Build the LIR the lowerer produces for `(fn (y) (+ x y))`, `x` captured.
///
/// The trap: env index and destination register are both small integers, and
/// here they cross — `LoadCaptureRaw` reads env index 1 into r0, then env index
/// 0 into r1. One map for both spaces makes the first load overwrite what the
/// second reads, and the function returns `y + y`.
fn make_capture_param_collision() -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name("cap_param_collision")
        .signal(Signal::errors())
        .num_captures(1)
        .num_locals(1) // one local slot for param copy
        .block(
            0,
            vec![
                // Copy param from env index 1 to local slot 0
                LirInstr::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 1, // param y
                },
                LirInstr::StoreLocal {
                    slot: 0,
                    src: Reg(0),
                },
                // Load capture from env index 0
                LirInstr::LoadCaptureRaw {
                    dst: Reg(1),
                    index: 0, // capture x
                },
                // Load param from local slot
                LirInstr::LoadLocal {
                    dst: Reg(2),
                    slot: 0,
                },
                // x + y
                LirInstr::binop(Reg(3), BinOp::Add, Reg(1), Reg(2)),
            ],
            Terminator::Return(Reg(3)),
        )
        .build()
}

#[test]
fn test_capture_param_collision() {
    let func = make_capture_param_collision();
    let context = lower::create_context();
    // num_captures=1, capture_types=0 (int), param_types=0 (int)
    let (mut module, _) =
        lower::lower_to_module(&context, &func, 1, 0, 0).expect("lowering should succeed");
    let pm = melior::pass::PassManager::new(&context);
    pm.add_pass(melior::pass::conversion::create_to_llvm());
    pm.run(&mut module).expect("LLVM conversion should succeed");
    let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);
    // capture(x)=10, param(y)=5 → should return 15
    let mut cap: i64 = 10;
    let mut arg: i64 = 5;
    let mut result: i64 = 0;
    unsafe {
        engine
            .invoke_packed(
                "cap_param_collision",
                &mut [
                    &mut cap as *mut i64 as *mut (),
                    &mut arg as *mut i64 as *mut (),
                    &mut result as *mut i64 as *mut (),
                ],
            )
            .unwrap();
    }
    assert_eq!(result, 15, "capture(10) + param(5) should be 15");
}
