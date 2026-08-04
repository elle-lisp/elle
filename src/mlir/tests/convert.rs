use super::*;

/// Build LIR: fn(x) { return float(x) }
fn make_int_to_float() -> LirFunction {
    make_convert("int_to_float", ConvOp::IntToFloat)
}

/// Build LIR: fn(x) { return int(x) }
fn make_float_to_int() -> LirFunction {
    make_convert("float_to_int", ConvOp::FloatToInt)
}

/// Build LIR: fn(x) { return <op>(x) } — the shape both conversions share.
fn make_convert(name: &str, op: ConvOp) -> LirFunction {
    LirFixture::new(Arity::Exact(1))
        .name(name)
        .signal(Signal::errors())
        .block(
            0,
            vec![
                LirInstr::LoadCaptureRaw {
                    dst: Reg(0),
                    index: 0,
                },
                LirInstr::Convert {
                    dst: Reg(1),
                    op,
                    src: Reg(0),
                },
            ],
            Terminator::Return(Reg(1)),
        )
        .build()
}

#[test]
fn test_lower_int_to_float() {
    let mlir_text = lower_to_mlir(&make_int_to_float()).expect("lowering should succeed");
    assert!(
        mlir_text.contains("arith.sitofp"),
        "should contain arith.sitofp: {}",
        mlir_text
    );
}

#[test]
fn test_lower_float_to_int() {
    // Float arg via param_types bitmask
    let context = lower::create_context();
    let (module, _) = lower::lower_to_module(&context, &make_float_to_int(), 0, 0, 1)
        .expect("lowering should succeed");
    let mlir_text = module.as_operation().to_string();
    assert!(
        mlir_text.contains("arith.fptosi"),
        "should contain arith.fptosi: {}",
        mlir_text
    );
}

#[test]
fn test_execute_int_to_float() {
    let result = mlir_call(&make_int_to_float(), &[42]).expect("execution should succeed");
    assert_eq!(result, 42.0f64.to_bits() as i64);
}

#[test]
fn test_execute_float_to_int() {
    let func = make_float_to_int();
    let bits = 3.7f64.to_bits() as i64;
    // Need to call with param_types=1 to mark arg as float
    let context = lower::create_context();
    let (mut module, _) =
        lower::lower_to_module(&context, &func, 0, 0, 1).expect("lowering should succeed");
    let pm = melior::pass::PassManager::new(&context);
    pm.add_pass(melior::pass::conversion::create_to_llvm());
    pm.run(&mut module).expect("LLVM conversion should succeed");
    let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);
    let mut arg: i64 = bits;
    let mut result: i64 = 0;
    unsafe {
        engine
            .invoke_packed(
                "float_to_int",
                &mut [
                    &mut arg as *mut i64 as *mut (),
                    &mut result as *mut i64 as *mut (),
                ],
            )
            .unwrap();
    }
    assert_eq!(result, 3, "fptosi(3.7) should be 3");
}

#[test]
fn test_spirv_int_to_float() {
    let func = make_int_to_float();
    let spirv_bytes = lower_to_spirv(&func, 256).expect("SPIR-V lowering should succeed");
    assert!(spirv_bytes.len() >= 20);
    assert_eq!(&spirv_bytes[0..4], &[0x03, 0x02, 0x23, 0x07]);
}
