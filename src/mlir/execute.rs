//! JIT execution of MLIR-lowered functions.
//!
//! Takes a GPU-eligible LirFunction, lowers it to MLIR, converts to
//! LLVM IR, and JIT-compiles it via the MLIR ExecutionEngine. The
//! result is a callable function pointer with C calling convention.

use crate::lir::LirFunction;
use melior::pass;

use super::lower::{create_context, lower_to_module};

/// JIT-compile a GPU-eligible LirFunction and call it with i64 arguments.
///
/// The function is lowered to MLIR (arith/func/cf), converted to LLVM
/// dialect, then JIT-compiled. Arguments and return value are raw i64.
pub fn mlir_call(lir: &LirFunction, args: &[i64]) -> Result<i64, String> {
    let context = create_context();
    let (mut module, _) = lower_to_module(&context, lir)?;

    // Convert arith/func/cf → LLVM dialect
    let pass_manager = pass::PassManager::new(&context);
    pass_manager.add_pass(pass::conversion::create_to_llvm());

    pass_manager
        .run(&mut module)
        .map_err(|_| "MLIR-to-LLVM conversion failed".to_string())?;

    // JIT compile
    let engine = melior::ExecutionEngine::new(&module, 2, &[], false, false);

    let func_name = lir.name.as_deref().unwrap_or("gpu_kernel");

    // invoke_packed expects pointers to args and result
    let mut arg_values: Vec<i64> = args.to_vec();
    let mut result: i64 = 0;

    let mut packed_args: Vec<*mut ()> = Vec::new();
    for arg in &mut arg_values {
        packed_args.push(arg as *mut i64 as *mut ());
    }
    packed_args.push(&mut result as *mut i64 as *mut ());

    unsafe {
        engine
            .invoke_packed(func_name, &mut packed_args)
            .map_err(|e| format!("MLIR execution failed: {:?}", e))?;
    }

    Ok(result)
}
