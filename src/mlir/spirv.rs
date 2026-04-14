//! Lower GPU-eligible LirFunction to SPIR-V bytes.
//!
//! Generates a compute kernel from a scalar LIR function by wrapping
//! it in a gpu.module with buffer I/O. The kernel loads from input
//! buffers at the global invocation ID, applies the function body,
//! and stores the result to an output buffer.
//!
//! Pipeline: LIR → MLIR gpu.module → spirv.module → SPIR-V bytes
//! (via mlir-translate subprocess for serialization).

use crate::lir::{BinOp, CmpOp, LirConst, LirFunction, LirInstr, Terminator, UnaryOp};
use std::io::Write;
use std::process::{Command, Stdio};

/// Path to mlir-translate (built with MLIR).
/// TODO: make this configurable via env var or config.
const MLIR_TRANSLATE: &str = concat!(env!("HOME"), "/git/tmp/mlir-install/bin/mlir-translate");

/// Lower a GPU-eligible LirFunction to SPIR-V bytes.
///
/// The function is wrapped in a compute kernel:
/// - For an N-ary function, the kernel takes N+1 buffers:
///   N input buffers + 1 output buffer
/// - Each workgroup thread loads from inputs at global_id,
///   applies the function, and stores to output at global_id
///
/// Returns the SPIR-V binary bytes, ready for Vulkan dispatch.
pub fn lower_to_spirv(lir: &LirFunction, workgroup_size: u32) -> Result<Vec<u8>, String> {
    let mlir_text = generate_gpu_module(lir, workgroup_size)?;
    let spirv_mlir = run_mlir_opt(&mlir_text)?;
    let spirv_module = extract_spirv_module(&spirv_mlir)?;
    serialize_spirv(&spirv_module)
}

/// Generate MLIR text for a gpu.module wrapping the LIR function.
fn generate_gpu_module(lir: &LirFunction, workgroup_size: u32) -> Result<String, String> {
    let num_params = lir.arity.fixed_params();
    let func_name = lir.name.as_deref().unwrap_or("gpu_kernel");

    // Buffer size is dynamic at runtime, but MLIR needs static shapes
    // for the conversion. Use a placeholder that gets replaced at dispatch.
    // For now, use a large fixed size — the actual dispatch will use
    // the right workgroup count.
    let buf_size = "?"; // dynamic dimension

    let mut out = String::new();

    // Module with GPU container and SPIR-V target env
    out.push_str("module attributes {\n");
    out.push_str("  gpu.container_module,\n");
    out.push_str("  spirv.target_env = #spirv.target_env<\n");
    out.push_str(
        "    #spirv.vce<v1.0, [Shader, Int64], [SPV_KHR_storage_buffer_storage_class]>,\n",
    );
    out.push_str("    #spirv.resource_limits<>>\n");
    out.push_str("} {\n");

    // GPU module
    out.push_str("  gpu.module @kernels {\n");

    // GPU function signature: one memref<Nxi32> per input + one for output
    // Entry point must be "main" — the Vulkan plugin hardcodes this name.
    out.push_str("    gpu.func @main(");
    for i in 0..num_params {
        out.push_str(&format!("%buf{}: memref<{}xi64>, ", i, buf_size));
    }
    out.push_str(&format!("%out: memref<{}xi64>)\n", buf_size));
    out.push_str(&format!(
        "      kernel attributes {{ spirv.entry_point_abi = #spirv.entry_point_abi<workgroup_size = [{}, 1, 1]>}} {{\n",
        workgroup_size
    ));

    // Load global ID
    out.push_str("      %gid = gpu.thread_id x\n");

    // Load from input buffers
    for i in 0..num_params {
        out.push_str(&format!(
            "      %arg{} = memref.load %buf{}[%gid] : memref<{}xi64>\n",
            i, i, buf_size
        ));
    }

    // Lower the function body (only single-block for now)
    if lir.blocks.len() != 1 {
        return Err("SPIR-V lowering currently supports single-block functions only".to_string());
    }
    let block = &lir.blocks[0];
    let mut reg_names: std::collections::HashMap<u32, String> = std::collections::HashMap::new();

    // Map param registers to loaded values
    for si in &block.instructions {
        match &si.instr {
            LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                if (*index as usize) < num_params {
                    reg_names.insert(dst.0, format!("%arg{}", index));
                }
            }
            LirInstr::Const { dst, value } => {
                let name = format!("%c{}", dst.0);
                let val = match value {
                    LirConst::Int(n) => {
                        format!("      {} = arith.constant {} : i64\n", name, *n)
                    }
                    LirConst::Bool(b) => format!(
                        "      {} = arith.constant {} : i64\n",
                        name,
                        if *b { 1 } else { 0 }
                    ),
                    LirConst::Nil => format!("      {} = arith.constant 0 : i64\n", name),
                    _ => return Err(format!("unsupported constant for SPIR-V: {:?}", value)),
                };
                out.push_str(&val);
                reg_names.insert(dst.0, name);
            }
            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let name = format!("%r{}", dst.0);
                let lv = reg_names
                    .get(&lhs.0)
                    .ok_or_else(|| format!("undef r{}", lhs.0))?;
                let rv = reg_names
                    .get(&rhs.0)
                    .ok_or_else(|| format!("undef r{}", rhs.0))?;
                let op_name = match op {
                    BinOp::Add => "arith.addi",
                    BinOp::Sub => "arith.subi",
                    BinOp::Mul => "arith.muli",
                    BinOp::Div => "arith.divsi",
                    BinOp::Rem => "arith.remsi",
                    BinOp::BitAnd => "arith.andi",
                    BinOp::BitOr => "arith.ori",
                    BinOp::BitXor => "arith.xori",
                    BinOp::Shl => "arith.shli",
                    BinOp::Shr => "arith.shrsi",
                };
                out.push_str(&format!(
                    "      {} = {} {}, {} : i64\n",
                    name, op_name, lv, rv
                ));
                reg_names.insert(dst.0, name);
            }
            LirInstr::StoreLocal { .. } | LirInstr::LoadLocal { .. } => {
                // Skip local slot ops for single-block kernels — they're just SSA copies
                if let LirInstr::StoreLocal { slot, src } = &si.instr {
                    if let Some(name) = reg_names.get(&src.0) {
                        reg_names.insert(*slot as u32, name.clone());
                    }
                }
                if let LirInstr::LoadLocal { dst, slot } = &si.instr {
                    if let Some(name) = reg_names.get(&(*slot as u32)) {
                        reg_names.insert(dst.0, name.clone());
                    }
                }
            }
            _ => return Err(format!("unsupported SPIR-V instruction: {:?}", si.instr)),
        }
    }

    // Store result to output buffer
    let result_reg = match &block.terminator.terminator {
        Terminator::Return(reg) => reg.0,
        _ => return Err("SPIR-V kernel must end with Return".to_string()),
    };
    let result_name = reg_names
        .get(&result_reg)
        .ok_or_else(|| format!("undefined result reg r{}", result_reg))?;
    out.push_str(&format!(
        "      memref.store {}, %out[%gid] : memref<{}xi64>\n",
        result_name, buf_size
    ));
    out.push_str("      gpu.return\n");
    out.push_str("    }\n"); // gpu.func
    out.push_str("  }\n"); // gpu.module
    out.push_str("}\n"); // module

    Ok(out)
}

/// Run mlir-opt to convert GPU → SPIR-V dialect.
fn run_mlir_opt(mlir_text: &str) -> Result<String, String> {
    let mlir_opt = MLIR_TRANSLATE.replace("mlir-translate", "mlir-opt");
    let mut child = Command::new(&mlir_opt)
        .args([
            "--convert-gpu-to-spirv",
            "--spirv-lower-abi-attrs",
            "--spirv-update-vce",
        ])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run mlir-opt: {}", e))?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(mlir_text.as_bytes())
        .map_err(|e| format!("failed to write to mlir-opt: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("mlir-opt failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "mlir-opt failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    String::from_utf8(output.stdout).map_err(|e| format!("mlir-opt output not utf8: {}", e))
}

/// Extract the spirv.module text from mlir-opt output.
fn extract_spirv_module(mlir_text: &str) -> Result<String, String> {
    let start = mlir_text
        .find("spirv.module")
        .ok_or("no spirv.module in mlir-opt output")?;

    let bytes = mlir_text.as_bytes();
    let mut depth = 0i32;
    let mut end = start;
    for (i, &b) in bytes[start..].iter().enumerate() {
        if b == b'{' {
            depth += 1;
        } else if b == b'}' {
            depth -= 1;
            if depth == 0 {
                end = start + i + 1;
                break;
            }
        }
    }

    Ok(mlir_text[start..end].to_string())
}

/// Serialize SPIR-V dialect text to binary bytes via mlir-translate.
fn serialize_spirv(spirv_text: &str) -> Result<Vec<u8>, String> {
    let mut child = Command::new(MLIR_TRANSLATE)
        .args(["--no-implicit-module", "--serialize-spirv"])
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .spawn()
        .map_err(|e| format!("failed to run mlir-translate: {}", e))?;

    child
        .stdin
        .take()
        .unwrap()
        .write_all(spirv_text.as_bytes())
        .map_err(|e| format!("failed to write to mlir-translate: {}", e))?;

    let output = child
        .wait_with_output()
        .map_err(|e| format!("mlir-translate failed: {}", e))?;

    if !output.status.success() {
        return Err(format!(
            "mlir-translate failed: {}",
            String::from_utf8_lossy(&output.stderr)
        ));
    }

    Ok(output.stdout)
}
