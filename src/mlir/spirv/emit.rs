// audited: 2026-09-06
// docs/impl/spirv.md
//! Emitting the GPU-eligible LIR subset as MLIR text for the SPIR-V pipeline.
//!
//! Registers are named strings rather than SSA values, because the output is
//! text `mlir-translate` reads rather than a module built in memory.

use super::*;

/// Helper: emit a sitofp promotion for a register from i64 to f64.
pub(super) fn emit_promote(name: &str, src: &str, indent: &str, out: &mut String) {
    out.push_str(&format!(
        "{indent}{name} = arith.sitofp {src} : i64 to f64\n"
    ));
}
pub(super) fn emit_block_instructions(
    instructions: &[crate::lir::SpannedInstr],
    env: &mut SsaEnv,
    num_params: usize,
    block_idx: usize,
    indent: &str,
    out: &mut String,
) -> Result<(), String> {
    for si in instructions {
        match &si.instr {
            LirInstr::LoadCaptureRaw { dst, index } | LirInstr::LoadCapture { dst, index } => {
                if (*index as usize) < num_params {
                    env.reg_names.insert(*dst, format!("%arg{}", index));
                    env.reg_types.insert(*dst, ScalarType::Int);
                }
            }
            LirInstr::Const { dst, value } => {
                let name = format!("%c{}_{}", block_idx, dst.0);
                match value {
                    LirConst::Float(f) => {
                        // Format with enough precision to round-trip.
                        let s = format!("{:.17e}", f);
                        out.push_str(&format!("{indent}{name} = arith.constant {s} : f64\n"));
                        env.reg_names.insert(*dst, name);
                        env.reg_types.insert(*dst, ScalarType::Float);
                    }
                    _ => {
                        let n = match value {
                            LirConst::Int(n) => *n,
                            LirConst::Bool(b) => i64::from(*b),
                            LirConst::Nil => 0,
                            _ => {
                                return Err(format!("unsupported constant for SPIR-V: {:?}", value))
                            }
                        };
                        out.push_str(&format!("{indent}{name} = arith.constant {n} : i64\n"));
                        env.reg_names.insert(*dst, name);
                        env.reg_types.insert(*dst, ScalarType::Int);
                    }
                }
            }
            LirInstr::BinOp {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => {
                let lv = env
                    .reg_names
                    .get(lhs)
                    .ok_or_else(|| format!("undef r{}", lhs.0))?
                    .clone();
                let rv = env
                    .reg_names
                    .get(rhs)
                    .ok_or_else(|| format!("undef r{}", rhs.0))?
                    .clone();
                // A proof settles both operand types; the local walk reads an
                // untyped register as an integer by default (docs/impl/lir.md).
                let (lt, rt) = if proof.is_int() {
                    (ScalarType::Int, ScalarType::Int)
                } else {
                    (
                        env.reg_types.get(lhs).copied().unwrap_or(ScalarType::Int),
                        env.reg_types.get(rhs).copied().unwrap_or(ScalarType::Int),
                    )
                };

                let is_bitwise = matches!(
                    op,
                    BinOp::BitAnd | BinOp::BitOr | BinOp::BitXor | BinOp::Shl | BinOp::Shr
                );
                if is_bitwise && (lt == ScalarType::Float || rt == ScalarType::Float) {
                    return Err("bitwise ops on float operands not supported in SPIR-V".to_string());
                }

                // Promote mixed operands: int → float via sitofp
                let (eff_lv, eff_rv, result_type) = match (lt, rt) {
                    (ScalarType::Int, ScalarType::Int) => (lv, rv, ScalarType::Int),
                    (ScalarType::Float, ScalarType::Float) => (lv, rv, ScalarType::Float),
                    (ScalarType::Int, ScalarType::Float) => {
                        let pname = format!("%prom{}_{}_l", block_idx, dst.0);
                        emit_promote(&pname, &lv, indent, out);
                        (pname, rv, ScalarType::Float)
                    }
                    (ScalarType::Float, ScalarType::Int) => {
                        let pname = format!("%prom{}_{}_r", block_idx, dst.0);
                        emit_promote(&pname, &rv, indent, out);
                        (lv, pname, ScalarType::Float)
                    }
                    // Bool operands: treat as Int (0/1)
                    (ScalarType::Bool, ScalarType::Bool) => (lv, rv, ScalarType::Int),
                    (ScalarType::Bool, other) | (other, ScalarType::Bool) => (lv, rv, other),
                };

                let name = format!("%r{}_{}", block_idx, dst.0);
                if result_type == ScalarType::Float {
                    let op_name = match op {
                        BinOp::Add => "arith.addf",
                        BinOp::Sub => "arith.subf",
                        BinOp::Mul => "arith.mulf",
                        BinOp::Div => "arith.divf",
                        BinOp::Rem => "arith.remf",
                        _ => unreachable!("bitwise on float rejected above"),
                    };
                    out.push_str(&format!(
                        "{indent}{name} = {op_name} {eff_lv}, {eff_rv} : f64\n"
                    ));
                } else {
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
                        "{indent}{name} = {op_name} {eff_lv}, {eff_rv} : i64\n"
                    ));
                }
                env.reg_names.insert(*dst, name);
                env.reg_types.insert(*dst, result_type);
            }
            LirInstr::Compare {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => {
                let lv = env
                    .reg_names
                    .get(lhs)
                    .ok_or_else(|| format!("undef r{}", lhs.0))?
                    .clone();
                let rv = env
                    .reg_names
                    .get(rhs)
                    .ok_or_else(|| format!("undef r{}", rhs.0))?
                    .clone();
                let (lt, rt) = if proof.is_int() {
                    (ScalarType::Int, ScalarType::Int)
                } else {
                    (
                        env.reg_types.get(lhs).copied().unwrap_or(ScalarType::Int),
                        env.reg_types.get(rhs).copied().unwrap_or(ScalarType::Int),
                    )
                };
                let use_float = lt == ScalarType::Float || rt == ScalarType::Float;

                let cmp_i1 = format!("%cmpi1_{}_{}", block_idx, dst.0);
                let ext_i64 = format!("%cmp{}_{}", block_idx, dst.0);

                if use_float {
                    // Promote mixed operands for float comparison
                    let (eff_lv, eff_rv) = match (lt, rt) {
                        (ScalarType::Float, ScalarType::Float) => (lv, rv),
                        (ScalarType::Int, ScalarType::Float) => {
                            let pname = format!("%cprom{}_{}_l", block_idx, dst.0);
                            emit_promote(&pname, &lv, indent, out);
                            (pname, rv)
                        }
                        (ScalarType::Float, ScalarType::Int) => {
                            let pname = format!("%cprom{}_{}_r", block_idx, dst.0);
                            emit_promote(&pname, &rv, indent, out);
                            (lv, pname)
                        }
                        _ => unreachable!(),
                    };
                    let pred = match op {
                        CmpOp::Eq => "oeq",
                        CmpOp::Ne => "one",
                        CmpOp::Lt => "olt",
                        CmpOp::Le => "ole",
                        CmpOp::Gt => "ogt",
                        CmpOp::Ge => "oge",
                    };
                    out.push_str(&format!(
                        "{indent}{cmp_i1} = arith.cmpf {pred}, {eff_lv}, {eff_rv} : f64\n"
                    ));
                } else {
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
                }
                out.push_str(&format!(
                    "{indent}{ext_i64} = arith.extui {cmp_i1} : i1 to i64\n"
                ));
                env.reg_names.insert(*dst, ext_i64);
                env.reg_types.insert(*dst, ScalarType::Int);
            }
            LirInstr::UnaryOp {
                dst,
                op,
                src,
                proof,
            } => {
                let sv = env
                    .reg_names
                    .get(src)
                    .ok_or_else(|| format!("undef r{}", src.0))?
                    .clone();
                let st = if proof.is_int() {
                    ScalarType::Int
                } else {
                    env.reg_types.get(src).copied().unwrap_or(ScalarType::Int)
                };
                let name = format!("%u{}_{}", block_idx, dst.0);

                match op {
                    UnaryOp::Neg => {
                        if st == ScalarType::Float {
                            out.push_str(&format!("{indent}{name} = arith.negf {sv} : f64\n"));
                            env.reg_types.insert(*dst, ScalarType::Float);
                        } else {
                            let zero = format!("%neg_z{}_{}", block_idx, dst.0);
                            out.push_str(&format!("{indent}{zero} = arith.constant 0 : i64\n"));
                            out.push_str(&format!(
                                "{indent}{name} = arith.subi {zero}, {sv} : i64\n"
                            ));
                            env.reg_types.insert(*dst, ScalarType::Int);
                        }
                    }
                    UnaryOp::Not => {
                        // Truthiness: compare to zero, result is always Int
                        if st == ScalarType::Float {
                            let zero = format!("%not_z{}_{}", block_idx, dst.0);
                            let cmp = format!("%not_c{}_{}", block_idx, dst.0);
                            out.push_str(&format!("{indent}{zero} = arith.constant 0.0 : f64\n"));
                            out.push_str(&format!(
                                "{indent}{cmp} = arith.cmpf oeq, {sv}, {zero} : f64\n"
                            ));
                            out.push_str(&format!(
                                "{indent}{name} = arith.extui {cmp} : i1 to i64\n"
                            ));
                        } else {
                            let zero = format!("%not_z{}_{}", block_idx, dst.0);
                            let cmp = format!("%not_c{}_{}", block_idx, dst.0);
                            out.push_str(&format!("{indent}{zero} = arith.constant 0 : i64\n"));
                            out.push_str(&format!(
                                "{indent}{cmp} = arith.cmpi eq, {sv}, {zero} : i64\n"
                            ));
                            out.push_str(&format!(
                                "{indent}{name} = arith.extui {cmp} : i1 to i64\n"
                            ));
                        }
                        env.reg_types.insert(*dst, ScalarType::Int);
                    }
                    UnaryOp::BitNot => {
                        if st == ScalarType::Float {
                            return Err(
                                "bitwise not on float operand not supported in SPIR-V".to_string()
                            );
                        }
                        let neg1 = format!("%bn_m1{}_{}", block_idx, dst.0);
                        out.push_str(&format!("{indent}{neg1} = arith.constant -1 : i64\n"));
                        out.push_str(&format!("{indent}{name} = arith.xori {sv}, {neg1} : i64\n"));
                        env.reg_types.insert(*dst, ScalarType::Int);
                    }
                }
                env.reg_names.insert(*dst, name);
            }
            LirInstr::Convert { dst, op, src } => {
                let sv = env
                    .reg_names
                    .get(src)
                    .ok_or_else(|| format!("undef r{}", src.0))?
                    .clone();
                let st = env.reg_types.get(src).copied().unwrap_or(ScalarType::Int);
                let name = format!("%conv{}_{}", block_idx, dst.0);
                match op {
                    ConvOp::IntToFloat => {
                        if st == ScalarType::Float {
                            // Identity
                            env.reg_names.insert(*dst, sv);
                            env.reg_types.insert(*dst, ScalarType::Float);
                        } else {
                            out.push_str(&format!(
                                "{indent}{name} = arith.sitofp {sv} : i64 to f64\n"
                            ));
                            env.reg_names.insert(*dst, name);
                            env.reg_types.insert(*dst, ScalarType::Float);
                        }
                    }
                    ConvOp::FloatToInt => {
                        if st == ScalarType::Int {
                            // Identity
                            env.reg_names.insert(*dst, sv);
                            env.reg_types.insert(*dst, ScalarType::Int);
                        } else {
                            out.push_str(&format!(
                                "{indent}{name} = arith.fptosi {sv} : f64 to i64\n"
                            ));
                            env.reg_names.insert(*dst, name);
                            env.reg_types.insert(*dst, ScalarType::Int);
                        }
                    }
                }
            }
            LirInstr::StoreLocal { slot, src } => {
                // Slot key, not register key: the store names a *local*, and
                // must not disturb the register whose id collides numerically.
                if let Some(name) = env.reg_names.get(src).cloned() {
                    env.slot_names.insert(SlotId::new(*slot as u32), name);
                    if let Some(t) = env.reg_types.get(src).copied() {
                        env.slot_types.insert(SlotId::new(*slot as u32), t);
                    }
                }
            }
            LirInstr::LoadLocal { dst, slot } => {
                if let Some(name) = env.slot_names.get(&SlotId::new(*slot as u32)).cloned() {
                    env.reg_names.insert(*dst, name);
                    if let Some(t) = env.slot_types.get(&SlotId::new(*slot as u32)).copied() {
                        env.reg_types.insert(*dst, t);
                    }
                }
            }
            // Value-targeted region refcounts: no-ops on unboxed scalars
            // (the eligibility whitelist admits nothing heap-valued).
            LirInstr::IncrefValueRegion { .. } | LirInstr::DecrefValueRegion { .. } => {}
            _ => return Err(format!("unsupported SPIR-V instruction: {:?}", si.instr)),
        }
    }
    Ok(())
}
mod multiblock;
pub(crate) use multiblock::emit_multiblock;
