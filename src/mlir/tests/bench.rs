use super::*;

/// Build LIR: fn(a, b) { return a * b + a }
fn make_mul_add() -> LirFunction {
    LirFixture::new(Arity::Exact(2))
        .name("mul_add")
        .signal(Signal::errors())
        .block(
            0,
            vec![
                LirInstr::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::LoadCaptureRaw {
                    dst: Reg(1),
                    index: 1,
                },
                LirInstr::BinOp {
                    dst: Reg(2),
                    op: BinOp::Mul,
                    lhs: Reg(0),
                    rhs: Reg(1),
                },
                LirInstr::BinOp {
                    dst: Reg(3),
                    op: BinOp::Add,
                    lhs: Reg(2),
                    rhs: Reg(0),
                },
            ],
            Terminator::Return(Reg(3)),
        )
        .build()
}

#[test]
fn test_execute_mul_add() {
    // a * b + a = 3 * 7 + 3 = 24
    let result = mlir_call(&make_mul_add(), &[3, 7]).expect("execution should succeed");
    assert_eq!(result, 24);
}

#[test]
fn test_spirv_mul_add() {
    let func = make_mul_add();
    let spirv_bytes = lower_to_spirv(&func, 64).expect("SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}

#[test]
fn bench_mlir() {
    use super::lower::{create_context, lower_to_module};
    use std::time::Instant;

    let func = make_mul_add();
    let n = 1_000_000;

    // ── MLIR: break down each phase ─────────────────────────
    let start = Instant::now();
    let context = create_context();
    let ctx_time = start.elapsed();

    let start = Instant::now();
    let (mut module, _) = lower_to_module(&context, &func, 0, 0, 0).unwrap();
    let lower_time = start.elapsed();

    let start = Instant::now();
    let pm = melior::pass::PassManager::new(&context);
    pm.add_pass(melior::pass::conversion::create_to_llvm());
    pm.run(&mut module).unwrap();
    let convert_time = start.elapsed();

    let start = Instant::now();
    let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);
    let jit_time = start.elapsed();

    let start = Instant::now();
    for i in 0..n {
        let mut a: i64 = i;
        let mut b: i64 = 7;
        let mut result: i64 = 0;
        unsafe {
            engine
                .invoke_packed(
                    "mul_add",
                    &mut [
                        &mut a as *mut i64 as *mut (),
                        &mut b as *mut i64 as *mut (),
                        &mut result as *mut i64 as *mut (),
                    ],
                )
                .unwrap();
        }
        assert_eq!(result, i * 7 + i);
    }
    let mlir_exec_time = start.elapsed();

    // ── Cranelift: compile only (execution needs VM context) ─
    let start = Instant::now();
    let compiler = crate::jit::JitCompiler::new().unwrap();
    let cranelift_init = start.elapsed();

    let start = Instant::now();
    let _jit_code = compiler.compile(&func, None, vec![]).unwrap();
    let cranelift_compile = start.elapsed();

    eprintln!();
    eprintln!("── mul_add(a,b) = a*b+a, {} exec iterations ──", n);
    eprintln!();
    eprintln!("  MLIR:");
    eprintln!("    context creation:  {:?}", ctx_time);
    eprintln!("    lower LIR→MLIR:    {:?}", lower_time);
    eprintln!("    convert →LLVM:     {:?}", convert_time);
    eprintln!("    LLVM JIT compile:  {:?}", jit_time);
    eprintln!(
        "    compile total:     {:?}",
        ctx_time + lower_time + convert_time + jit_time
    );
    eprintln!(
        "    exec:              {:?} ({:?}/call)",
        mlir_exec_time,
        mlir_exec_time / n as u32
    );
    eprintln!();
    eprintln!("  Cranelift:");
    eprintln!("    init:              {:?}", cranelift_init);
    eprintln!("    compile:           {:?}", cranelift_compile);
    eprintln!(
        "    compile total:     {:?}",
        cranelift_init + cranelift_compile
    );
}
