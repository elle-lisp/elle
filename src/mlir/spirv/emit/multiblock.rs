//! Multi-block (control-flow) SPIR-V emission and block-result helpers.

use super::*;

pub(crate) fn emit_multiblock(
    lir: &LirFunction,
    env: &mut SsaEnv,
    num_params: usize,
    buf_size: &str,
    indent: &str,
    out: &mut String,
) -> Result<(), String> {
    let mut block_idx = 0;
    while block_idx < lir.blocks.len() {
        let block = &lir.blocks[block_idx];
        emit_block_instructions(&block.instructions, env, num_params, block_idx, indent, out)?;

        match &block.terminator.terminator {
            Terminator::Return(reg) => {
                let result = env.reg_names.get(reg).ok_or("undef result in return")?;
                let rt = env.reg_types.get(reg).copied().unwrap_or(ScalarType::Int);
                let store_val = if rt == ScalarType::Float {
                    let bc = format!("%mret_bc_{}", block_idx);
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
                let cond_raw = env.reg_names.get(cond).ok_or("undef cond")?.clone();
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
                            env,
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

                // scf.if always yields i64; float branches bitcast before yield.
                let if_result = format!("%if_result_{}", block_idx);
                out.push_str(&format!(
                    "{indent}{if_result} = scf.if {cond_val} -> (i64) {{\n"
                ));

                let inner = format!("{indent}  ");
                let mut then_env = env.clone();
                emit_block_instructions(
                    &then_block.instructions,
                    &mut then_env,
                    num_params,
                    then_idx,
                    &inner,
                    out,
                )?;
                let then_val = then_env
                    .reg_names
                    .get(&then_result)
                    .ok_or("undef then result")?;
                let then_ty = then_env
                    .reg_types
                    .get(&then_result)
                    .copied()
                    .unwrap_or(ScalarType::Int);
                if then_ty == ScalarType::Float {
                    let bc = format!("%then_bc_{}", block_idx);
                    out.push_str(&format!(
                        "{inner}{bc} = arith.bitcast {then_val} : f64 to i64\n"
                    ));
                    out.push_str(&format!("{inner}scf.yield {bc} : i64\n"));
                } else {
                    out.push_str(&format!("{inner}scf.yield {then_val} : i64\n"));
                }
                out.push_str(&format!("{indent}}} else {{\n"));

                let mut else_env = env.clone();
                emit_block_instructions(
                    &else_block.instructions,
                    &mut else_env,
                    num_params,
                    else_idx,
                    &inner,
                    out,
                )?;
                let else_val = else_env
                    .reg_names
                    .get(&else_result)
                    .ok_or("undef else result")?;
                let else_ty = else_env
                    .reg_types
                    .get(&else_result)
                    .copied()
                    .unwrap_or(ScalarType::Int);
                if else_ty == ScalarType::Float {
                    let bc = format!("%else_bc_{}", block_idx);
                    out.push_str(&format!(
                        "{inner}{bc} = arith.bitcast {else_val} : f64 to i64\n"
                    ));
                    out.push_str(&format!("{inner}scf.yield {bc} : i64\n"));
                } else {
                    out.push_str(&format!("{inner}scf.yield {else_val} : i64\n"));
                }
                out.push_str(&format!("{indent}}}\n"));

                // The merged value lands in the *slot* the branches stored to,
                // so the merge block's LoadLocal reads it back. Only the name
                // is rebound; the slot keeps its established type (the i64
                // yield matches the Int that scalar slots carry here).
                if let Some(store_slot) = find_store_slot(then_block) {
                    env.slot_names.insert(store_slot, if_result.clone());
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
/// The register a branch block's last `StoreLocal` reads from — the value the
/// `scf.if` arm yields. Typed `Reg` so it can only index the register maps.
pub(super) fn find_block_result(block: &crate::lir::BasicBlock) -> Result<Reg, String> {
    for si in block.instructions.iter().rev() {
        if let LirInstr::StoreLocal { src, .. } = &si.instr {
            return Ok(*src);
        }
    }
    Err("branch block has no StoreLocal".to_string())
}
/// The slot a branch block's last `StoreLocal` writes to — where the merged
/// `scf.if` result is rebound. Typed `SlotId` so it can only index the slot
/// maps, never the register maps.
pub(super) fn find_store_slot(block: &crate::lir::BasicBlock) -> Option<SlotId> {
    for si in block.instructions.iter().rev() {
        if let LirInstr::StoreLocal { slot, .. } = &si.instr {
            return Some(SlotId::new(*slot as u32));
        }
    }
    None
}
pub(super) fn emit_if_return(
    lir: &LirFunction,
    env: &mut SsaEnv,
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
        Terminator::Return(r) => *r,
        _ => return Err("expected return in then".to_string()),
    };
    let else_ret = match &else_block.terminator.terminator {
        Terminator::Return(r) => *r,
        _ => return Err("expected return in else".to_string()),
    };

    // scf.if always yields i64; float returns bitcast before yield.
    let if_result = format!("%if_ret_{}", idx.entry_idx);
    out.push_str(&format!(
        "{indent}{if_result} = scf.if {cond_val} -> (i64) {{\n"
    ));

    let inner = format!("{indent}  ");

    let mut then_env = env.clone();
    emit_block_instructions(
        &then_block.instructions,
        &mut then_env,
        num_params,
        idx.then_idx,
        &inner,
        out,
    )?;
    let then_val = then_env.reg_names.get(&then_ret).ok_or("undef then ret")?;
    let then_ty = then_env
        .reg_types
        .get(&then_ret)
        .copied()
        .unwrap_or(ScalarType::Int);
    if then_ty == ScalarType::Float {
        let bc = format!("%tret_bc_{}", idx.entry_idx);
        out.push_str(&format!(
            "{inner}{bc} = arith.bitcast {then_val} : f64 to i64\n"
        ));
        out.push_str(&format!("{inner}scf.yield {bc} : i64\n"));
    } else {
        out.push_str(&format!("{inner}scf.yield {then_val} : i64\n"));
    }
    out.push_str(&format!("{indent}}} else {{\n"));

    let mut else_env = env.clone();
    emit_block_instructions(
        &else_block.instructions,
        &mut else_env,
        num_params,
        idx.else_idx,
        &inner,
        out,
    )?;
    let else_val = else_env.reg_names.get(&else_ret).ok_or("undef else ret")?;
    let else_ty = else_env
        .reg_types
        .get(&else_ret)
        .copied()
        .unwrap_or(ScalarType::Int);
    if else_ty == ScalarType::Float {
        let bc = format!("%eret_bc_{}", idx.entry_idx);
        out.push_str(&format!(
            "{inner}{bc} = arith.bitcast {else_val} : f64 to i64\n"
        ));
        out.push_str(&format!("{inner}scf.yield {bc} : i64\n"));
    } else {
        out.push_str(&format!("{inner}scf.yield {else_val} : i64\n"));
    }
    out.push_str(&format!("{indent}}}\n"));

    out.push_str(&format!(
        "{indent}memref.store {if_result}, %out[%gid] : memref<{buf_size}xi64>\n"
    ));
    out.push_str(&format!("{indent}gpu.return\n"));
    Ok(())
}
