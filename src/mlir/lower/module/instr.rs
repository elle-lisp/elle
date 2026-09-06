// audited: 2026-09-06
// docs/impl/mlir.md
//! Per-instruction MLIR emission for the GPU-eligible LIR subset.
//!
//! One arm per supported `LirInstr`, appending arith/memref ops to the current
//! block. Register/type bookkeeping lives in [`LowerCtx`]; the emitted op order
//! is identical to the original single-function lowering.

use super::*;

/// Lower a single LIR instruction into `block`, updating `ctx`.
///
/// `entry_block` is block 0, used only for the `LoadCapture` fallback that
/// reads a raw MLIR argument when `env_vals` is unexpectedly missing an entry.
pub(super) fn lower_instr<'c, 'a>(
    ctx: &mut LowerCtx<'c, 'a>,
    block: &'a Block<'c>,
    entry_block: &'a Block<'c>,
    si: &crate::lir::SpannedInstr,
) -> Result<(), String> {
    let context = ctx.context;
    let location = ctx.location;
    let i64_type = ctx.i64_type;
    let f64_type = ctx.f64_type;

    match &si.instr {
        LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
            // Env layout: [captures..., params...].
            // MLIR arguments mirror this layout, so index maps directly
            // to the MLIR block argument index.
            let idx = *index as usize;
            if idx < ctx.total_args {
                // Use env_vals (never clobbered by dst writes) to look
                // up the (possibly bitcast) MLIR value and its type.
                if let Some(&(val, t)) = ctx.env_vals.get(&EnvIndex::new(idx as u32)) {
                    ctx.regs.insert(*dst, val);
                    ctx.types.insert(*dst, t);
                } else {
                    // Fallback: shouldn't happen if env_vals was populated
                    ctx.regs
                        .insert(*dst, entry_block.argument(idx).unwrap().into());
                    ctx.types.insert(*dst, ScalarType::Int);
                }
            }
        }
        LirInstr::Const { dst, value } => match value {
            LirConst::Float(f) => {
                let op = arith::constant(
                    context,
                    FloatAttribute::new(context, f64_type, *f).into(),
                    location,
                );
                let op_ref = block.append_operation(op);
                ctx.regs.insert(*dst, op_ref.result(0).unwrap().into());
                ctx.types.insert(*dst, ScalarType::Float);
            }
            _ => {
                let (n, scalar_type) = match value {
                    LirConst::Int(n) => (*n, ScalarType::Int),
                    LirConst::Bool(b) => (i64::from(*b), ScalarType::Bool),
                    LirConst::Nil => (0i64, ScalarType::Int),
                    _ => return Err(format!("unsupported constant: {:?}", value)),
                };
                let op =
                    arith::constant(context, IntegerAttribute::new(i64_type, n).into(), location);
                let op_ref = block.append_operation(op);
                ctx.regs.insert(*dst, op_ref.result(0).unwrap().into());
                ctx.types.insert(*dst, scalar_type);
            }
        },
        LirInstr::BinOp {
            dst,
            op,
            lhs,
            rhs,
            proof,
        } => {
            let lv = *ctx
                .regs
                .get(lhs)
                .ok_or_else(|| format!("undefined reg r{}", lhs.0))?;
            let rv = *ctx
                .regs
                .get(rhs)
                .ok_or_else(|| format!("undefined reg r{}", rhs.0))?;
            // A proof settles both operand types; the local walk reads an
            // untyped register as an integer by default (docs/impl/lir.md).
            let (lt, rt) = if proof.is_int() {
                (ScalarType::Int, ScalarType::Int)
            } else {
                (
                    ctx.types.get(lhs).copied().unwrap_or(ScalarType::Int),
                    ctx.types.get(rhs).copied().unwrap_or(ScalarType::Int),
                )
            };

            let is_bitwise = matches!(
                op,
                BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
            );
            if is_bitwise && (lt == ScalarType::Float || rt == ScalarType::Float) {
                return Err("bitwise ops on float operands not supported".to_string());
            }

            // Promote mixed operands: int → float via sitofp
            let (eff_lv, eff_rv, result_type) = match (lt, rt) {
                (ScalarType::Int, ScalarType::Int) => (lv, rv, ScalarType::Int),
                (ScalarType::Float, ScalarType::Float) => (lv, rv, ScalarType::Float),
                (ScalarType::Int, ScalarType::Float) => {
                    let p = block.append_operation(arith::sitofp(lv, f64_type, location));
                    (p.result(0).unwrap().into(), rv, ScalarType::Float)
                }
                (ScalarType::Float, ScalarType::Int) => {
                    let p = block.append_operation(arith::sitofp(rv, f64_type, location));
                    (lv, p.result(0).unwrap().into(), ScalarType::Float)
                }
                // Bool operands: treat as Int (0/1)
                (ScalarType::Bool, ScalarType::Bool) => (lv, rv, ScalarType::Int),
                (ScalarType::Bool, other) | (other, ScalarType::Bool) => {
                    // Promote bool to the other operand's type
                    (lv, rv, other)
                }
            };

            let mlir_op = if result_type == ScalarType::Float {
                match op {
                    BinOp::Add => arith::addf(eff_lv, eff_rv, location),
                    BinOp::Sub => arith::subf(eff_lv, eff_rv, location),
                    BinOp::Mul => arith::mulf(eff_lv, eff_rv, location),
                    BinOp::Div => arith::divf(eff_lv, eff_rv, location),
                    BinOp::Rem => arith::remf(eff_lv, eff_rv, location),
                    _ => unreachable!("bitwise on float rejected above"),
                }
            } else {
                match op {
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
                }
            };
            let op_ref = block.append_operation(mlir_op);
            ctx.regs.insert(*dst, op_ref.result(0).unwrap().into());
            ctx.types.insert(*dst, result_type);
        }
        LirInstr::Compare {
            dst,
            op,
            lhs,
            rhs,
            proof,
        } => {
            let lv = *ctx
                .regs
                .get(lhs)
                .ok_or_else(|| format!("undefined reg r{}", lhs.0))?;
            let rv = *ctx
                .regs
                .get(rhs)
                .ok_or_else(|| format!("undefined reg r{}", rhs.0))?;
            let (lt, rt) = if proof.is_int() {
                (ScalarType::Int, ScalarType::Int)
            } else {
                (
                    ctx.types.get(lhs).copied().unwrap_or(ScalarType::Int),
                    ctx.types.get(rhs).copied().unwrap_or(ScalarType::Int),
                )
            };
            let use_float = lt == ScalarType::Float || rt == ScalarType::Float;

            let op_ref = if use_float {
                // Promote mixed operands for float comparison
                let (eff_lv, eff_rv) = match (lt, rt) {
                    (ScalarType::Float, ScalarType::Float) => (lv, rv),
                    (ScalarType::Int, ScalarType::Float) => {
                        let p = block.append_operation(arith::sitofp(lv, f64_type, location));
                        (p.result(0).unwrap().into(), rv)
                    }
                    (ScalarType::Float, ScalarType::Int) => {
                        let p = block.append_operation(arith::sitofp(rv, f64_type, location));
                        (lv, p.result(0).unwrap().into())
                    }
                    _ => unreachable!(),
                };
                let pred = match op {
                    CmpOp::Eq => CmpfPredicate::Oeq,
                    CmpOp::Ne => CmpfPredicate::One,
                    CmpOp::Lt => CmpfPredicate::Olt,
                    CmpOp::Le => CmpfPredicate::Ole,
                    CmpOp::Gt => CmpfPredicate::Ogt,
                    CmpOp::Ge => CmpfPredicate::Oge,
                };
                block.append_operation(arith::cmpf(context, pred, eff_lv, eff_rv, location))
            } else {
                let pred = match op {
                    CmpOp::Eq => CmpiPredicate::Eq,
                    CmpOp::Ne => CmpiPredicate::Ne,
                    CmpOp::Lt => CmpiPredicate::Slt,
                    CmpOp::Le => CmpiPredicate::Sle,
                    CmpOp::Gt => CmpiPredicate::Sgt,
                    CmpOp::Ge => CmpiPredicate::Sge,
                };
                block.append_operation(arith::cmpi(context, pred, lv, rv, location))
            };
            // cmpi/cmpf returns i1; extend to i64 for consistency
            let i1_val: Value = op_ref.result(0).unwrap().into();
            let ext_ref = block.append_operation(arith::extui(i1_val, i64_type, location));
            ctx.regs.insert(*dst, ext_ref.result(0).unwrap().into());
            ctx.types.insert(*dst, ScalarType::Bool);
        }
        LirInstr::UnaryOp {
            dst,
            op,
            src,
            proof,
        } => {
            let sv = *ctx
                .regs
                .get(src)
                .ok_or_else(|| format!("undefined reg r{}", src.0))?;
            let src_type = if proof.is_int() {
                ScalarType::Int
            } else {
                ctx.types.get(src).copied().unwrap_or(ScalarType::Int)
            };
            let (result, result_type) = match op {
                UnaryOp::Neg => {
                    if src_type == ScalarType::Float {
                        let neg = block.append_operation(arith::negf(sv, location));
                        (neg.result(0).unwrap().into(), ScalarType::Float)
                    } else {
                        let zero = block.append_operation(arith::constant(
                            context,
                            IntegerAttribute::new(i64_type, 0).into(),
                            location,
                        ));
                        let zero_val: Value = zero.result(0).unwrap().into();
                        let sub = block.append_operation(arith::subi(zero_val, sv, location));
                        (sub.result(0).unwrap().into(), ScalarType::Int)
                    }
                }
                UnaryOp::Not => {
                    if src_type == ScalarType::Float {
                        // Truthiness: compare float to 0.0
                        let zero = block.append_operation(arith::constant(
                            context,
                            FloatAttribute::new(context, f64_type, 0.0).into(),
                            location,
                        ));
                        let zero_val: Value = zero.result(0).unwrap().into();
                        let cmp = block.append_operation(arith::cmpf(
                            context,
                            CmpfPredicate::Oeq,
                            sv,
                            zero_val,
                            location,
                        ));
                        let i1_val: Value = cmp.result(0).unwrap().into();
                        let ext = block.append_operation(arith::extui(i1_val, i64_type, location));
                        (ext.result(0).unwrap().into(), ScalarType::Int)
                    } else {
                        let zero = block.append_operation(arith::constant(
                            context,
                            IntegerAttribute::new(i64_type, 0).into(),
                            location,
                        ));
                        let zero_val: Value = zero.result(0).unwrap().into();
                        let cmp = block.append_operation(arith::cmpi(
                            context,
                            CmpiPredicate::Eq,
                            sv,
                            zero_val,
                            location,
                        ));
                        let i1_val: Value = cmp.result(0).unwrap().into();
                        let ext = block.append_operation(arith::extui(i1_val, i64_type, location));
                        (ext.result(0).unwrap().into(), ScalarType::Int)
                    }
                }
                UnaryOp::BitNot => {
                    if src_type == ScalarType::Float {
                        return Err("bitwise not on float operand not supported".to_string());
                    }
                    let neg1 = block.append_operation(arith::constant(
                        context,
                        IntegerAttribute::new(i64_type, -1).into(),
                        location,
                    ));
                    let neg1_val: Value = neg1.result(0).unwrap().into();
                    let xor = block.append_operation(arith::xori(sv, neg1_val, location));
                    (xor.result(0).unwrap().into(), ScalarType::Int)
                }
            };
            ctx.regs.insert(*dst, result);
            ctx.types.insert(*dst, result_type);
        }
        LirInstr::StoreLocal { slot, src } => {
            let val = *ctx
                .regs
                .get(src)
                .ok_or_else(|| format!("undefined reg r{} in StoreLocal", src.0))?;
            let src_type = ctx.types.get(src).copied().unwrap_or(ScalarType::Int);
            // Memref slots are always i64; bitcast f64 → i64 for storage
            let store_val = if src_type == ScalarType::Float {
                let bc = block.append_operation(arith::bitcast(val, i64_type, location));
                bc.result(0).unwrap().into()
            } else {
                val
            };
            let slot_ptr = *ctx
                .local_slots
                .get(&SlotId::new(*slot as u32))
                .ok_or_else(|| format!("unallocated local slot {}", slot))?;
            block.append_operation(memref::store(store_val, slot_ptr, &[], location));
            ctx.slot_types.insert(SlotId::new(*slot as u32), src_type);
        }
        LirInstr::LoadLocal { dst, slot } => {
            let slot_ptr = *ctx
                .local_slots
                .get(&SlotId::new(*slot as u32))
                .ok_or_else(|| format!("unallocated local slot {}", slot))?;
            let load_op = block.append_operation(memref::load(slot_ptr, &[], location));
            let loaded: Value = load_op.result(0).unwrap().into();
            let slot_ty = ctx
                .slot_types
                .get(&SlotId::new(*slot as u32))
                .copied()
                .unwrap_or(ScalarType::Int);
            // Memref slots are i64; bitcast i64 → f64 if slot holds a float
            let result = if slot_ty == ScalarType::Float {
                let bc = block.append_operation(arith::bitcast(loaded, f64_type, location));
                bc.result(0).unwrap().into()
            } else {
                loaded
            };
            ctx.regs.insert(*dst, result);
            ctx.types.insert(*dst, slot_ty);
        }
        LirInstr::Convert { dst, op, src } => {
            let sv = *ctx
                .regs
                .get(src)
                .ok_or_else(|| format!("undefined reg r{}", src.0))?;
            let src_type = ctx.types.get(src).copied().unwrap_or(ScalarType::Int);
            let (result, result_type) = match op {
                ConvOp::IntToFloat => {
                    if src_type == ScalarType::Float {
                        (sv, ScalarType::Float)
                    } else {
                        let conv = block.append_operation(arith::sitofp(sv, f64_type, location));
                        (conv.result(0).unwrap().into(), ScalarType::Float)
                    }
                }
                ConvOp::FloatToInt => {
                    if src_type == ScalarType::Int {
                        (sv, ScalarType::Int)
                    } else {
                        let conv = block.append_operation(arith::fptosi(sv, i64_type, location));
                        (conv.result(0).unwrap().into(), ScalarType::Int)
                    }
                }
            };
            ctx.regs.insert(*dst, result);
            ctx.types.insert(*dst, result_type);
        }
        // Value-targeted region refcounts: no-ops on unboxed
        // scalars (the eligibility whitelist admits nothing that
        // could hold a heap value).
        LirInstr::IncrefValueRegion { .. } | LirInstr::DecrefValueRegion { .. } => {}
        _ => return Err(format!("unsupported instruction: {:?}", si.instr)),
    }
    Ok(())
}
