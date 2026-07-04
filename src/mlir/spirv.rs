//! Lower GPU-eligible LirFunction to SPIR-V bytes.
//!
//! Generates a compute kernel from a scalar LIR function by wrapping
//! it in a gpu.module with buffer I/O. Uses scf.if for control flow.
//!
//! Pipeline: LIR → MLIR text → parse → pass pipeline → extract binary

use crate::lir::{BinOp, CmpOp, ConvOp, LirConst, LirFunction, LirInstr, Reg, Terminator, UnaryOp};

use super::lower::{ScalarType, SlotId};
use melior::ir::Module;
use melior::pass;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use super::lower::create_context;

mod emit;
use emit::*;

/// Lower a GPU-eligible LirFunction to SPIR-V bytes (creates fresh context).
pub fn lower_to_spirv(lir: &LirFunction, workgroup_size: u32) -> Result<Vec<u8>, String> {
    let context = create_context();
    lower_to_spirv_with_context(&context, lir, workgroup_size)
}

/// Lower a GPU-eligible LirFunction to SPIR-V bytes using a shared context.
pub fn lower_to_spirv_with_context(
    context: &melior::Context,
    lir: &LirFunction,
    workgroup_size: u32,
) -> Result<Vec<u8>, String> {
    let mlir_text = generate_gpu_module(lir, workgroup_size)?;
    let mut module = Module::parse(context, &mlir_text).ok_or("failed to parse generated MLIR")?;

    // Pass pipeline: convert standard dialects to SPIR-V inside gpu.module,
    // then convert gpu.module to spirv.module, then lower ABI/VCE.
    let pm = pass::PassManager::new(context);

    // Nest passes inside gpu.module
    let gpu_pm = pm.nested_under("gpu.module");
    gpu_pm.add_pass(pass::conversion::create_arith_to_spirv());
    gpu_pm.add_pass(pass::conversion::create_control_flow_to_spirv());
    gpu_pm.add_pass(pass::conversion::create_scf_to_spirv());
    gpu_pm.add_pass(pass::conversion::create_mem_ref_to_spirv());

    // Convert gpu.module → spirv.module
    pm.add_pass(pass::conversion::create_gpu_to_spirv());

    // Nest passes inside spirv.module
    let spirv_pm = pm.nested_under("spirv.module");
    spirv_pm.add_pass(pass::spirv::create_spirv_lower_abi_attributes_pass());
    spirv_pm.add_pass(pass::spirv::create_spirv_update_vce_pass());

    pm.run(&mut module)
        .map_err(|_| "SPIR-V conversion pass pipeline failed".to_string())?;

    // Extract spirv.module text and serialize to bytes.
    // The MLIR C API doesn't expose SPIR-V serialization directly,
    // so we use mlir-translate for the final step.
    let module_text = module.as_operation().to_string();
    let spirv_text = extract_spirv_module(&module_text)?;
    serialize_spirv(&spirv_text)
}

/// SSA-name and scalar-type environment threaded through emission.
///
/// Registers and local slots are held in **separate** maps keyed by distinct
/// types. LIR allocates the two from independent counters that both start at 0
/// (`next_reg` and `num_locals`), so a slot id and a register id routinely
/// collide numerically. Keying each namespace by its own type means a slot id
/// can never be used to read the register map (or vice-versa) — the invariant
/// is enforced by the compiler instead of by an unchecked numeric assumption.
#[derive(Clone, Default)]
struct SsaEnv {
    /// LIR register → MLIR SSA name (e.g. `"%r0_3"`).
    reg_names: HashMap<Reg, String>,
    /// LIR register → scalar type.
    reg_types: HashMap<Reg, ScalarType>,
    /// Local slot → MLIR SSA name.
    slot_names: HashMap<SlotId, String>,
    /// Local slot → scalar type.
    slot_types: HashMap<SlotId, ScalarType>,
}

/// Generate MLIR text for a gpu.module wrapping the LIR function.
pub(super) fn generate_gpu_module(
    lir: &LirFunction,
    workgroup_size: u32,
) -> Result<String, String> {
    if lir.num_captures > 0 {
        return Err("captures not supported in SPIR-V".to_string());
    }
    let num_params = lir.arity.fixed_params();
    let buf_size = "?";
    let indent = "      ";

    let mut out = String::new();

    // Module header
    out.push_str("module attributes {\n");
    out.push_str("  gpu.container_module,\n");
    out.push_str("  spirv.target_env = #spirv.target_env<\n");
    out.push_str(
        "    #spirv.vce<v1.0, [Shader, Int64, Float64], [SPV_KHR_storage_buffer_storage_class]>,\n",
    );
    out.push_str("    #spirv.resource_limits<>>\n");
    out.push_str("} {\n");
    out.push_str("  gpu.module @kernels {\n");

    // Function signature
    out.push_str("    gpu.func @main(");
    for i in 0..num_params {
        out.push_str(&format!("%buf{}: memref<{}xi64>, ", i, buf_size));
    }
    out.push_str(&format!("%out: memref<{}xi64>)\n", buf_size));
    out.push_str(&format!(
        "      kernel attributes {{ spirv.entry_point_abi = #spirv.entry_point_abi<workgroup_size = [{}, 1, 1]>}} {{\n",
        workgroup_size
    ));

    // Load global ID + input params
    out.push_str(&format!("{indent}%gid = gpu.thread_id x\n"));
    for i in 0..num_params {
        out.push_str(&format!(
            "{indent}%arg{i} = memref.load %buf{i}[%gid] : memref<{buf_size}xi64>\n"
        ));
    }

    let mut env = SsaEnv::default();

    if lir.blocks.len() == 1 {
        emit_block_instructions(
            &lir.blocks[0].instructions,
            &mut env,
            num_params,
            0,
            indent,
            &mut out,
        )?;
        let result_reg = match &lir.blocks[0].terminator.terminator {
            Terminator::Return(reg) => *reg,
            _ => return Err("SPIR-V kernel must end with Return".to_string()),
        };
        let result = env.reg_names.get(&result_reg).ok_or("undef result")?;
        let rt = env
            .reg_types
            .get(&result_reg)
            .copied()
            .unwrap_or(ScalarType::Int);
        // Float results: bitcast f64→i64 for the output buffer.
        let store_val = if rt == ScalarType::Float {
            let bc = "%ret_bc".to_string();
            out.push_str(&format!(
                "{indent}{bc} = arith.bitcast {result} : f64 to i64\n"
            ));
            bc
        } else {
            result.clone()
        };
        out.push_str(&format!(
            "{indent}memref.store {store_val}, %out[%gid] : memref<{buf_size}xi64>\n"
        ));
        out.push_str(&format!("{indent}gpu.return\n"));
    } else {
        emit_multiblock(lir, &mut env, num_params, buf_size, indent, &mut out)?;
    }

    out.push_str("    }\n");
    out.push_str("  }\n");
    out.push_str("}\n");

    Ok(out)
}

/// Indices into `lir.blocks` describing an `if` that returns directly.
struct IfReturn<'a> {
    entry_idx: usize,
    then_idx: usize,
    else_idx: usize,
    cond_val: &'a str,
    buf_size: &'a str,
    indent: &'a str,
}

/// Extract the spirv.module text from the converted MLIR.
fn extract_spirv_module(mlir_text: &str) -> Result<String, String> {
    let start = mlir_text.find("spirv.module").ok_or("no spirv.module")?;
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

/// Find the mlir-translate binary.
///
/// Search order:
/// 1. MLIR_TRANSLATE env var (explicit path)
/// 2. $MLIR_SYS_220_PREFIX/bin/mlir-translate (same install as melior)
/// 3. mlir-translate on $PATH
fn find_mlir_translate() -> Result<String, String> {
    if let Ok(path) = std::env::var("MLIR_TRANSLATE") {
        return Ok(path);
    }
    if let Ok(prefix) = std::env::var("MLIR_SYS_220_PREFIX") {
        let path = format!("{}/bin/mlir-translate", prefix);
        if std::path::Path::new(&path).exists() {
            return Ok(path);
        }
    }
    // Check PATH via `which`
    if let Ok(output) = Command::new("which").arg("mlir-translate").output() {
        if output.status.success() {
            let path = String::from_utf8_lossy(&output.stdout).trim().to_string();
            if !path.is_empty() {
                return Ok(path);
            }
        }
    }
    Err(
        "mlir-translate not found. Set MLIR_TRANSLATE or MLIR_SYS_220_PREFIX, or add to PATH."
            .to_string(),
    )
}

/// Serialize SPIR-V dialect text to binary bytes via mlir-translate.
fn serialize_spirv(spirv_text: &str) -> Result<Vec<u8>, String> {
    let mlir_translate = find_mlir_translate()?;
    let mut child = Command::new(&mlir_translate)
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
