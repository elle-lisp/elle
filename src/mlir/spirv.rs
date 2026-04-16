//! Lower GPU-eligible LirFunction to SPIR-V bytes.
//!
//! Generates a compute kernel from a scalar LIR function by wrapping
//! it in a gpu.module with buffer I/O. Uses scf.if for control flow.
//!
//! Pipeline: LIR → MLIR text → parse → pass pipeline → extract binary

use crate::lir::{BinOp, CmpOp, LirConst, LirFunction, LirInstr, Terminator};
use melior::ir::Module;
use melior::pass;
use std::collections::HashMap;
use std::io::Write;
use std::process::{Command, Stdio};

use super::lower::create_context;

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

/// Generate MLIR text for a gpu.module wrapping the LIR function.
fn generate_gpu_module(lir: &LirFunction, workgroup_size: u32) -> Result<String, String> {
    let num_params = lir.arity.fixed_params();
    let buf_size = "?";
    let indent = "      ";

    let mut out = String::new();

    // Module header
    out.push_str("module attributes {\n");
    out.push_str("  gpu.container_module,\n");
    out.push_str("  spirv.target_env = #spirv.target_env<\n");
    out.push_str(
        "    #spirv.vce<v1.0, [Shader, Int64], [SPV_KHR_storage_buffer_storage_class]>,\n",
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

    let mut regs: HashMap<u32, String> = HashMap::new();

    if lir.blocks.len() == 1 {
        emit_block_instructions(
            &lir.blocks[0].instructions,
            &mut regs,
            num_params,
            0,
            indent,
            &mut out,
        )?;
        let result_reg = match &lir.blocks[0].terminator.terminator {
            Terminator::Return(reg) => reg.0,
            _ => return Err("SPIR-V kernel must end with Return".to_string()),
        };
        let result = regs.get(&result_reg).ok_or("undef result")?;
        out.push_str(&format!(
            "{indent}memref.store {result}, %out[%gid] : memref<{buf_size}xi64>\n"
        ));
        out.push_str(&format!("{indent}gpu.return\n"));
    } else {
        emit_multiblock(lir, &mut regs, num_params, buf_size, indent, &mut out)?;
    }

    out.push_str("    }\n");
    out.push_str("  }\n");
    out.push_str("}\n");

    Ok(out)
}

fn emit_block_instructions(
    instructions: &[crate::lir::SpannedInstr],
    regs: &mut HashMap<u32, String>,
    num_params: usize,
    block_idx: usize,
    indent: &str,
    out: &mut String,
) -> Result<(), String> {
    for si in instructions {
        match &si.instr {
            LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                if (*index as usize) < num_params {
                    regs.insert(dst.0, format!("%arg{}", index));
                }
            }
            LirInstr::Const { dst, value } => {
                let name = format!("%c{}_{}", block_idx, dst.0);
                let n = match value {
                    LirConst::Int(n) => *n,
                    LirConst::Bool(b) => i64::from(*b),
                    LirConst::Nil => 0,
                    _ => return Err(format!("unsupported constant for SPIR-V: {:?}", value)),
                };
                out.push_str(&format!("{indent}{name} = arith.constant {n} : i64\n"));
                regs.insert(dst.0, name);
            }
            LirInstr::BinOp { dst, op, lhs, rhs } => {
                let name = format!("%r{}_{}", block_idx, dst.0);
                let lv = regs
                    .get(&lhs.0)
                    .ok_or_else(|| format!("undef r{}", lhs.0))?;
                let rv = regs
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
                out.push_str(&format!("{indent}{name} = {op_name} {lv}, {rv} : i64\n"));
                regs.insert(dst.0, name);
            }
            LirInstr::Compare { dst, op, lhs, rhs } => {
                // arith.cmpi produces i1; extend to i64 for consistency with
                // the rest of the i64-only LIR value domain (matches lower.rs).
                let cmp_i1 = format!("%cmpi1_{}_{}", block_idx, dst.0);
                let ext_i64 = format!("%cmp{}_{}", block_idx, dst.0);
                let lv = regs
                    .get(&lhs.0)
                    .ok_or_else(|| format!("undef r{}", lhs.0))?;
                let rv = regs
                    .get(&rhs.0)
                    .ok_or_else(|| format!("undef r{}", rhs.0))?;
                let pred = match op {
                    CmpOp::Eq => "eq",
                    CmpOp::Ne => "ne",
                    CmpOp::Lt => "slt",
                    CmpOp::Le => "sle",
                    CmpOp::Gt => "sgt",
                    CmpOp::Ge => "sge",
                };
                out.push_str(&format!(
                    "{indent}{cmp_i1} = arith.cmpi {pred}, {lv}, {rv} : i64\n"
                ));
                out.push_str(&format!(
                    "{indent}{ext_i64} = arith.extui {cmp_i1} : i1 to i64\n"
                ));
                regs.insert(dst.0, ext_i64);
            }
            LirInstr::StoreLocal { slot, src } => {
                if let Some(name) = regs.get(&src.0) {
                    regs.insert(*slot as u32, name.clone());
                }
            }
            LirInstr::LoadLocal { dst, slot } => {
                if let Some(name) = regs.get(&(*slot as u32)) {
                    regs.insert(dst.0, name.clone());
                }
            }
            _ => return Err(format!("unsupported SPIR-V instruction: {:?}", si.instr)),
        }
    }
    Ok(())
}

fn emit_multiblock(
    lir: &LirFunction,
    regs: &mut HashMap<u32, String>,
    num_params: usize,
    buf_size: &str,
    indent: &str,
    out: &mut String,
) -> Result<(), String> {
    let mut block_idx = 0;
    while block_idx < lir.blocks.len() {
        let block = &lir.blocks[block_idx];
        emit_block_instructions(
            &block.instructions,
            regs,
            num_params,
            block_idx,
            indent,
            out,
        )?;

        match &block.terminator.terminator {
            Terminator::Return(reg) => {
                let result = regs.get(&reg.0).ok_or("undef result in return")?;
                out.push_str(&format!(
                    "{indent}memref.store {result}, %out[%gid] : memref<{buf_size}xi64>\n"
                ));
                out.push_str(&format!("{indent}gpu.return\n"));
                break;
            }
            Terminator::Jump(label) => {
                block_idx = lir
                    .blocks
                    .iter()
                    .position(|b| b.label == *label)
                    .ok_or_else(|| format!("unknown jump target {}", label.0))?;
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let cond_raw = regs.get(&cond.0).ok_or("undef cond")?.clone();
                // Compare to zero for truthiness (0=false, nonzero=true).
                // scf.if expects i1; passing raw i64 fails verification.
                let cond_cmp = format!("%cond_ne_{}", block_idx);
                let cond_zero = format!("%cond_zero_{}", block_idx);
                out.push_str(&format!("{indent}{cond_zero} = arith.constant 0 : i64\n"));
                out.push_str(&format!(
                    "{indent}{cond_cmp} = arith.cmpi ne, {cond_raw}, {cond_zero} : i64\n"
                ));
                let cond_val = cond_cmp;
                let then_idx = lir
                    .blocks
                    .iter()
                    .position(|b| b.label == *then_label)
                    .ok_or("unknown then block")?;
                let else_idx = lir
                    .blocks
                    .iter()
                    .position(|b| b.label == *else_label)
                    .ok_or("unknown else block")?;

                let then_block = &lir.blocks[then_idx];
                let else_block = &lir.blocks[else_idx];

                let merge_label = match &then_block.terminator.terminator {
                    Terminator::Jump(l) => *l,
                    Terminator::Return(_) => {
                        return emit_if_return(
                            lir,
                            regs,
                            num_params,
                            IfReturn {
                                entry_idx: block_idx,
                                then_idx,
                                else_idx,
                                cond_val: &cond_val,
                                buf_size,
                                indent,
                            },
                            out,
                        );
                    }
                    _ => return Err("then block must end with Jump or Return".to_string()),
                };

                match &else_block.terminator.terminator {
                    Terminator::Jump(l) if *l == merge_label => {}
                    _ => return Err("else block must jump to same merge as then".to_string()),
                }

                let then_result = find_block_result(then_block)?;
                let else_result = find_block_result(else_block)?;

                let if_result = format!("%if_result_{}", block_idx);
                out.push_str(&format!(
                    "{indent}{if_result} = scf.if {cond_val} -> (i64) {{\n"
                ));

                let inner = format!("{indent}  ");
                let mut then_regs = regs.clone();
                emit_block_instructions(
                    &then_block.instructions,
                    &mut then_regs,
                    num_params,
                    then_idx,
                    &inner,
                    out,
                )?;
                let then_val = then_regs.get(&then_result).ok_or("undef then result")?;
                out.push_str(&format!("{inner}scf.yield {then_val} : i64\n"));
                out.push_str(&format!("{indent}}} else {{\n"));

                let mut else_regs = regs.clone();
                emit_block_instructions(
                    &else_block.instructions,
                    &mut else_regs,
                    num_params,
                    else_idx,
                    &inner,
                    out,
                )?;
                let else_val = else_regs.get(&else_result).ok_or("undef else result")?;
                out.push_str(&format!("{inner}scf.yield {else_val} : i64\n"));
                out.push_str(&format!("{indent}}}\n"));

                if let Some(store_slot) = find_store_slot(then_block) {
                    regs.insert(store_slot as u32, if_result.clone());
                }

                let merge_idx = lir
                    .blocks
                    .iter()
                    .position(|b| b.label == merge_label)
                    .ok_or("unknown merge block")?;
                block_idx = merge_idx;
            }
            _ => {
                return Err(format!(
                    "unsupported terminator: {:?}",
                    block.terminator.terminator
                ))
            }
        }
    }
    Ok(())
}

fn find_block_result(block: &crate::lir::BasicBlock) -> Result<u32, String> {
    for si in block.instructions.iter().rev() {
        if let LirInstr::StoreLocal { src, .. } = &si.instr {
            return Ok(src.0);
        }
    }
    Err("branch block has no StoreLocal".to_string())
}

fn find_store_slot(block: &crate::lir::BasicBlock) -> Option<u16> {
    for si in block.instructions.iter().rev() {
        if let LirInstr::StoreLocal { slot, .. } = &si.instr {
            return Some(*slot);
        }
    }
    None
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

fn emit_if_return(
    lir: &LirFunction,
    regs: &mut HashMap<u32, String>,
    num_params: usize,
    idx: IfReturn<'_>,
    out: &mut String,
) -> Result<(), String> {
    let cond_val = idx.cond_val;
    let buf_size = idx.buf_size;
    let indent = idx.indent;
    let then_block = &lir.blocks[idx.then_idx];
    let else_block = &lir.blocks[idx.else_idx];

    let then_ret = match &then_block.terminator.terminator {
        Terminator::Return(r) => r.0,
        _ => return Err("expected return in then".to_string()),
    };
    let else_ret = match &else_block.terminator.terminator {
        Terminator::Return(r) => r.0,
        _ => return Err("expected return in else".to_string()),
    };

    let if_result = format!("%if_ret_{}", idx.entry_idx);
    out.push_str(&format!(
        "{indent}{if_result} = scf.if {cond_val} -> (i64) {{\n"
    ));

    let inner = format!("{indent}  ");

    let mut then_regs = regs.clone();
    emit_block_instructions(
        &then_block.instructions,
        &mut then_regs,
        num_params,
        idx.then_idx,
        &inner,
        out,
    )?;
    let then_val = then_regs.get(&then_ret).ok_or("undef then ret")?;
    out.push_str(&format!("{inner}scf.yield {then_val} : i64\n"));
    out.push_str(&format!("{indent}}} else {{\n"));

    let mut else_regs = regs.clone();
    emit_block_instructions(
        &else_block.instructions,
        &mut else_regs,
        num_params,
        idx.else_idx,
        &inner,
        out,
    )?;
    let else_val = else_regs.get(&else_ret).ok_or("undef else ret")?;
    out.push_str(&format!("{inner}scf.yield {else_val} : i64\n"));
    out.push_str(&format!("{indent}}}\n"));

    out.push_str(&format!(
        "{indent}memref.store {if_result}, %out[%gid] : memref<{buf_size}xi64>\n"
    ));
    out.push_str(&format!("{indent}gpu.return\n"));
    Ok(())
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
