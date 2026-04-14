//! Lower GPU-eligible LirFunction to MLIR.
//!
//! Produces an MLIR module using the arith, func, and cf dialects.
//! Only handles the GPU-safe instruction subset — no heap allocation,
//! closures, function calls, or signal emission.

use crate::lir::{BinOp, LirConst, LirFunction, LirInstr, Terminator};
use melior::dialect::{arith, cf, func, DialectRegistry};
use melior::ir::attribute::{IntegerAttribute, StringAttribute, TypeAttribute};
use melior::ir::operation::OperationLike;
use melior::ir::r#type::{FunctionType, IntegerType};
use melior::ir::{Block, BlockLike, Location, Module, Region, RegionLike, Type, Value};
use melior::Context;
use std::collections::HashMap;

/// Lower a GPU-eligible LirFunction to MLIR text (for debugging/testing).
pub fn lower_to_mlir(lir: &LirFunction) -> Result<String, String> {
    let context = Context::new();
    let registry = DialectRegistry::new();
    melior::utility::register_all_dialects(&registry);
    context.append_dialect_registry(&registry);
    context.load_all_available_dialects();

    let location = Location::unknown(&context);
    let module = Module::new(location);

    let i64_type: Type = IntegerType::new(&context, 64).into();
    let num_params = lir.arity.fixed_params();

    // Build function signature: (i64, i64, ...) -> i64
    let param_types: Vec<Type> = (0..num_params).map(|_| i64_type).collect();
    let func_type = FunctionType::new(&context, &param_types, &[i64_type]);

    let func_name = lir.name.as_deref().unwrap_or("gpu_kernel");

    // Create function body region
    let region = Region::new();

    // Map LIR labels to MLIR block indices
    let mut label_to_idx: HashMap<u32, usize> = HashMap::new();
    let mut blocks: Vec<Block> = Vec::new();

    for (i, lir_block) in lir.blocks.iter().enumerate() {
        let block = if i == 0 {
            // Entry block gets function parameters
            Block::new(
                &param_types
                    .iter()
                    .map(|t| (*t, location))
                    .collect::<Vec<_>>(),
            )
        } else {
            Block::new(&[])
        };
        label_to_idx.insert(lir_block.label.0, i);
        blocks.push(block);
    }

    // Lower instructions in each block
    for (block_idx, lir_block) in lir.blocks.iter().enumerate() {
        let block = &blocks[block_idx];

        // Register map: LIR Reg → MLIR Value
        let mut regs: HashMap<u32, Value> = HashMap::new();

        // Entry block: map param indices to block arguments
        if block_idx == 0 {
            for i in 0..num_params {
                regs.insert(i as u32, block.argument(i).unwrap().into());
            }
        }

        for si in &lir_block.instructions {
            match &si.instr {
                LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                    // In GPU context (num_captures == 0), these are parameter loads.
                    if block_idx == 0 && (*index as usize) < num_params {
                        regs.insert(dst.0, block.argument(*index as usize).unwrap().into());
                    }
                }
                LirInstr::Const { dst, value } => {
                    let n = match value {
                        LirConst::Int(n) => *n,
                        LirConst::Bool(b) => {
                            if *b {
                                1i64
                            } else {
                                0i64
                            }
                        }
                        LirConst::Nil => 0i64,
                        LirConst::Float(f) => f.to_bits() as i64,
                        _ => return Err(format!("unsupported constant: {:?}", value)),
                    };
                    let op = arith::constant(
                        &context,
                        IntegerAttribute::new(i64_type, n).into(),
                        location,
                    );
                    let op_ref = block.append_operation(op);
                    regs.insert(dst.0, op_ref.result(0).unwrap().into());
                }
                LirInstr::BinOp { dst, op, lhs, rhs } => {
                    let lv = *regs
                        .get(&lhs.0)
                        .ok_or_else(|| format!("undefined reg r{}", lhs.0))?;
                    let rv = *regs
                        .get(&rhs.0)
                        .ok_or_else(|| format!("undefined reg r{}", rhs.0))?;
                    let mlir_op = match op {
                        BinOp::Add => arith::addi(lv, rv, location),
                        BinOp::Sub => arith::subi(lv, rv, location),
                        BinOp::Mul => arith::muli(lv, rv, location),
                        BinOp::Div => arith::divsi(lv, rv, location),
                        BinOp::Rem => arith::remsi(lv, rv, location),
                        BinOp::BitAnd => arith::andi(lv, rv, location),
                        BinOp::BitOr => arith::ori(lv, rv, location),
                        BinOp::BitXor => arith::xori(lv, rv, location),
                        BinOp::Shl => arith::shli(lv, rv, location),
                        BinOp::Shr => arith::shrsi(lv, rv, location),
                    };
                    let op_ref = block.append_operation(mlir_op);
                    regs.insert(dst.0, op_ref.result(0).unwrap().into());
                }
                LirInstr::LoadLocal { dst, slot } => {
                    if let Some(&val) = regs.get(&(*slot as u32)) {
                        regs.insert(dst.0, val);
                    }
                }
                LirInstr::StoreLocal { slot, src } => {
                    if let Some(&val) = regs.get(&src.0) {
                        regs.insert(*slot as u32, val);
                    }
                }
                _ => return Err(format!("unsupported instruction: {:?}", si.instr)),
            }
        }

        // Lower terminator
        match &lir_block.terminator.terminator {
            Terminator::Return(reg) => {
                let val = *regs
                    .get(&reg.0)
                    .ok_or_else(|| format!("undefined reg r{} in return", reg.0))?;
                block.append_operation(func::r#return(&[val], location));
            }
            Terminator::Jump(label) => {
                let target_idx = label_to_idx
                    .get(&label.0)
                    .ok_or_else(|| format!("unknown label {}", label.0))?;
                block.append_operation(cf::br(&blocks[*target_idx], &[], location));
            }
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => {
                let cond_val = *regs
                    .get(&cond.0)
                    .ok_or_else(|| format!("undefined reg r{} in branch", cond.0))?;
                let then_idx = *label_to_idx
                    .get(&then_label.0)
                    .ok_or_else(|| format!("unknown then label {}", then_label.0))?;
                let else_idx = *label_to_idx
                    .get(&else_label.0)
                    .ok_or_else(|| format!("unknown else label {}", else_label.0))?;
                block.append_operation(cf::cond_br(
                    &context,
                    cond_val,
                    &blocks[then_idx],
                    &blocks[else_idx],
                    &[],
                    &[],
                    location,
                ));
            }
            _ => {
                return Err(format!(
                    "unsupported terminator: {:?}",
                    lir_block.terminator.terminator
                ))
            }
        }
    }

    // Move blocks into region
    for block in blocks {
        region.append_block(block);
    }

    // Create the function operation
    let func_op = func::func(
        &context,
        StringAttribute::new(&context, func_name),
        TypeAttribute::new(func_type.into()),
        region,
        &[],
        location,
    );
    module.body().append_operation(func_op);

    // Verify and return text
    if !module.as_operation().verify() {
        return Err("MLIR verification failed".to_string());
    }

    Ok(module.as_operation().to_string())
}
