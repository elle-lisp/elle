// audited: 2026-10-06
// src/lir/AGENTS.md
//! Compact human-readable display for LIR instructions and terminators.
//!
//! The Debug format is verbose Rust struct syntax. This module provides
//! a compact format designed for CFG visualization:
//!   `Const { dst: Reg(0), value: Int(42) }` → `r0 ← 42`
//!   `BinOp { dst: Reg(2), op: Add, lhs: Reg(0), rhs: Reg(1) }` → `r2 ← r0 + r1`

use super::code::{ConstRef, InstrRef};
use super::types::*;
use std::fmt;

/// The suffix an operation carrying a proof about its operands is printed with.
/// An unproven operation gets none, so the common case reads unchanged.
fn proof_mark(proof: OperandProof) -> &'static str {
    match proof {
        OperandProof::Unproven => "",
        OperandProof::Int => " :int",
    }
}

// ── Reg and Label ───────────────────────────────────────────────────

impl fmt::Display for Reg {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "r{}", self.0)
    }
}

impl fmt::Display for Label {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "block{}", self.0)
    }
}

// ── Operators ───────────────────────────────────────────────────────

impl fmt::Display for BinOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            BinOp::Add => "+",
            BinOp::Sub => "-",
            BinOp::Mul => "*",
            BinOp::Div => "/",
            BinOp::Rem => "%",
            BinOp::BitAnd => "&",
            BinOp::BitOr => "|",
            BinOp::BitXor => "^",
            BinOp::Shl => "<<",
            BinOp::Shr => ">>",
        })
    }
}

impl fmt::Display for UnaryOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            UnaryOp::Neg => "-",
            UnaryOp::Not => "!",
            UnaryOp::BitNot => "~",
        })
    }
}

impl fmt::Display for CmpOp {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(match self {
            CmpOp::Eq => "=",
            CmpOp::Ne => "≠",
            CmpOp::Lt => "<",
            CmpOp::Le => "≤",
            CmpOp::Gt => ">",
            CmpOp::Ge => "≥",
        })
    }
}

// ── ConstRef ────────────────────────────────────────────────────────

impl fmt::Display for ConstRef {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            ConstRef::Nil => f.write_str("nil"),
            ConstRef::EmptyList => f.write_str("()"),
            ConstRef::Bool(true) => f.write_str("true"),
            ConstRef::Bool(false) => f.write_str("false"),
            ConstRef::Int(n) => write!(f, "{}", n),
            ConstRef::Float(n) => write!(f, "{}", n),
            ConstRef::Symbol(sid) => write!(f, "sym({})", sid.0),
            ConstRef::Keyword(hash) => write!(f, "kw({:#x})", hash),
        }
    }
}

// ── InstrRef ────────────────────────────────────────────────────────

/// Format helper: display a list of registers as comma-separated.
fn fmt_regs(regs: &[Reg], f: &mut fmt::Formatter<'_>) -> fmt::Result {
    for (i, r) in regs.iter().enumerate() {
        if i > 0 {
            f.write_str(", ")?;
        }
        write!(f, "{}", r)?;
    }
    Ok(())
}

impl fmt::Display for InstrRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // === Constants ===
            InstrRef::Const { dst, value } => write!(f, "{} ← {}", dst, value),
            InstrRef::ValueConst { dst, value } => write!(f, "{} ← val({})", dst, value),
            InstrRef::MaterializeConst {
                dst,
                template,
                region,
            } => {
                write!(
                    f,
                    "{} ← materialize({:?}) @r{}",
                    dst,
                    template.decode(),
                    region
                )
            }

            // === Variables ===
            InstrRef::LoadLocal { dst, slot } => write!(f, "{} ← local[{}]", dst, slot),
            InstrRef::StoreLocal { slot, src } => write!(f, "local[{}] ← {}", slot, src),
            InstrRef::StoreLocalRefcounted { slot, src } => {
                write!(f, "local[{}] ←rc {}", slot, src)
            }
            InstrRef::LoadCapture { dst, index } => write!(f, "{} ← cap[{}]", dst, index),
            InstrRef::LoadCaptureRaw { dst, index } => {
                write!(f, "{} ← cap[{}] (raw)", dst, index)
            }
            InstrRef::StoreCapture { index, src } => write!(f, "cap[{}] ← {}", index, src),

            // === Closures ===
            InstrRef::MakeClosure { dst, captures, .. } => {
                write!(f, "{} ← closure(", dst)?;
                fmt_regs(captures, f)?;
                f.write_str(")")
            }
            InstrRef::LoadSelf { dst } => write!(f, "{} ← self", dst),

            // === Function Calls ===
            InstrRef::Call {
                dst,
                func,
                args,
                arity_checked,
                ..
            }
            | InstrRef::SuspendingCall {
                dst,
                func,
                args,
                arity_checked,
                ..
            } => {
                write!(f, "{} ← {}(", dst, func)?;
                fmt_regs(args, f)?;
                if *arity_checked {
                    f.write_str(") ✓")
                } else {
                    f.write_str(")")
                }
            }
            InstrRef::TailCall {
                func,
                args,
                arity_checked,
                ..
            } => {
                write!(f, "tailcall {}(", func)?;
                fmt_regs(args, f)?;
                if *arity_checked {
                    f.write_str(") ✓")
                } else {
                    f.write_str(")")
                }
            }

            // === Data Construction ===
            InstrRef::List {
                dst, head, tail, ..
            } => {
                write!(f, "{} ← pair({}, {})", dst, head, tail)
            }
            InstrRef::MakeArrayMut { dst, elements, .. } => {
                write!(f, "{} ← array(", dst)?;
                fmt_regs(elements, f)?;
                f.write_str(")")
            }
            InstrRef::First { dst, pair } => write!(f, "{} ← first({})", dst, pair),
            InstrRef::Rest { dst, pair } => write!(f, "{} ← rest({})", dst, pair),

            // === Primitive Operations ===
            InstrRef::BinOp {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => {
                write!(f, "{} ← {} {} {}{}", dst, lhs, op, rhs, proof_mark(*proof))
            }
            InstrRef::UnaryOp {
                dst,
                op,
                src,
                proof,
            } => write!(f, "{} ← {}{}{}", dst, op, src, proof_mark(*proof)),
            InstrRef::Convert { dst, op, src } => {
                let name = match op {
                    ConvOp::IntToFloat => "float",
                    ConvOp::FloatToInt => "int",
                };
                write!(f, "{} ← {}({})", dst, name, src)
            }
            InstrRef::Compare {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => {
                write!(f, "{} ← {} {} {}{}", dst, lhs, op, rhs, proof_mark(*proof))
            }

            // === Type Checks ===
            InstrRef::IsNil { dst, src } => write!(f, "{} ← nil?({})", dst, src),
            InstrRef::IsPair { dst, src } => write!(f, "{} ← pair?({})", dst, src),
            InstrRef::IsArray { dst, src } => write!(f, "{} ← tuple?({})", dst, src),
            InstrRef::IsArrayMut { dst, src } => write!(f, "{} ← array?({})", dst, src),
            InstrRef::IsStruct { dst, src } => write!(f, "{} ← struct?({})", dst, src),
            InstrRef::IsStructMut { dst, src } => write!(f, "{} ← @struct?({})", dst, src),
            InstrRef::ArrayMutLen { dst, src } => write!(f, "{} ← len({})", dst, src),

            // === Box Operations ===
            InstrRef::MakeCaptureCell { dst, value, .. } => write!(f, "{} ← lbox({})", dst, value),
            InstrRef::LoadCaptureCell { dst, cell } => write!(f, "{} ← deref({})", dst, cell),
            InstrRef::StoreCaptureCell { cell, value } => write!(f, "deref({}) ← {}", cell, value),

            // === Destructuring ===
            InstrRef::MatchFail { dst, src } => write!(f, "{} ← match-fail!({})", dst, src),
            InstrRef::FirstDestructure { dst, src } => write!(f, "{} ← first!({})", dst, src),
            InstrRef::RestDestructure { dst, src } => write!(f, "{} ← rest!({})", dst, src),
            InstrRef::ArrayMutRefDestructure { dst, src, index } => {
                write!(f, "{} ← {}[{}]!", dst, src, index)
            }
            InstrRef::ArrayMutSliceFrom { dst, src, index } => {
                write!(f, "{} ← {}[{}..]", dst, src, index)
            }
            InstrRef::StructGetOrNil { dst, src, key } => {
                write!(f, "{} ← {}.{}?", dst, src, key)
            }
            InstrRef::StructGetDestructure { dst, src, key } => {
                write!(f, "{} ← {}.{}!", dst, src, key)
            }
            InstrRef::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                let keys: Vec<String> = exclude_keys.iter().map(|k| format!("{}", k)).collect();
                write!(f, "{} ← rest({}, excl=[{}])", dst, src, keys.join(", "))
            }

            // === Silent destructuring (parameter context) ===
            InstrRef::FirstOrNil { dst, src } => write!(f, "{} ← first?({})", dst, src),
            InstrRef::RestOrNil { dst, src } => write!(f, "{} ← rest?({})", dst, src),
            InstrRef::ArrayMutRefOrNil { dst, src, index } => {
                write!(f, "{} ← {}[{}]?", dst, src, index)
            }

            // === Fibers ===
            InstrRef::LoadResumeValue { dst } => write!(f, "{} ← resume-val", dst),

            // === Runtime Eval ===
            InstrRef::Eval { dst, expr, env } => {
                write!(f, "{} ← eval({}, {})", dst, expr, env)
            }

            // === Splice Support ===
            InstrRef::ArrayMutExtend { dst, array, source } => {
                write!(f, "{} ← extend({}, {})", dst, array, source)
            }
            InstrRef::ArrayMutPush { dst, array, value } => {
                write!(f, "{} ← push({}, {})", dst, array, value)
            }
            InstrRef::CallArrayMut {
                dst,
                func,
                args,
                args_region,
                ..
            } => {
                write!(
                    f,
                    "{} ← {}(;{}) [args-region {}]",
                    dst, func, args, args_region
                )
            }
            InstrRef::TailCallArrayMut {
                func,
                args,
                args_region,
                ..
            } => {
                write!(
                    f,
                    "tailcall {}(;{}) [args-region {}]",
                    func, args, args_region
                )
            }

            // === Allocation Regions ===
            InstrRef::IncrefRegion { region_id } => write!(f, "incref-region {region_id}"),
            InstrRef::DecrefRegion { region_id } => write!(f, "decref-region {region_id}"),
            InstrRef::DecrefValueRegion { src } => write!(f, "decref-value-region {src}"),
            InstrRef::DecrefCellRegion { src } => write!(f, "decref-cell-region {src}"),
            InstrRef::IncrefValueRegion { src } => write!(f, "incref-value-region {src}"),
            InstrRef::AdoptRegion { parent, child } => {
                write!(f, "adopt-region parent={parent} child={child}")
            }
            InstrRef::AdoptCellRegion { parent, child } => {
                write!(f, "adopt-cell-region parent={parent} child={child}")
            }
            InstrRef::AdoptIntoActivation { child } => {
                write!(f, "adopt-into-activation {child}")
            }
            InstrRef::FreeRegionGroup { members } => {
                write!(f, "free-region-group(")?;
                fmt_regs(members, f)?;
                f.write_str(")")
            }
            InstrRef::AssertRegionMatches { region_id, src } => {
                write!(f, "assert-region-matches {region_id} {src}")
            }
            // === Dynamic Parameters ===
            InstrRef::PushParamFrame { pairs } => {
                write!(f, "push-param-frame(")?;
                for (i, pair) in pairs.chunks(2).enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}={}", pair[0], pair[1])?;
                }
                write!(f, ")")
            }
            InstrRef::PopParamFrame => f.write_str("pop-param-frame"),
            InstrRef::IsSet { dst, src } => write!(f, "{} = is-set {}", dst, src),
            InstrRef::IsSetMut { dst, src } => write!(f, "{} = is-set-mut {}", dst, src),

            // New type predicates
            InstrRef::IsEmpty { dst, src } => write!(f, "{} ← empty?({})", dst, src),
            InstrRef::IsBool { dst, src } => write!(f, "{} ← bool?({})", dst, src),
            InstrRef::IsInt { dst, src } => write!(f, "{} ← int?({})", dst, src),
            InstrRef::IsFloat { dst, src } => write!(f, "{} ← float?({})", dst, src),
            InstrRef::IsString { dst, src } => write!(f, "{} ← string?({})", dst, src),
            InstrRef::IsKeyword { dst, src } => write!(f, "{} ← keyword?({})", dst, src),
            InstrRef::IsSymbolCheck { dst, src } => write!(f, "{} ← symbol?({})", dst, src),
            InstrRef::IsBytes { dst, src } => write!(f, "{} ← bytes?({})", dst, src),
            InstrRef::IsBox { dst, src } => write!(f, "{} ← box?({})", dst, src),
            InstrRef::IsClosure { dst, src } => write!(f, "{} ← closure?({})", dst, src),
            InstrRef::IsFiber { dst, src } => write!(f, "{} ← fiber?({})", dst, src),
            InstrRef::TypeOf { dst, src } => write!(f, "{} ← type-of({})", dst, src),

            // Data access
            InstrRef::Length { dst, src } => write!(f, "{} ← length({})", dst, src),
            InstrRef::Get { dst, obj, key } => write!(f, "{} ← get({}, {})", dst, obj, key),
            InstrRef::Put { dst, obj, key, val } => {
                write!(f, "{} ← put({}, {}, {})", dst, obj, key, val)
            }
            InstrRef::Del { dst, obj, key } => write!(f, "{} ← del({}, {})", dst, obj, key),
            InstrRef::Has { dst, obj, key } => write!(f, "{} ← has?({}, {})", dst, obj, key),
            InstrRef::IntrPush { dst, array, value } => {
                write!(f, "{} ← push({}, {})", dst, array, value)
            }
            InstrRef::IntrStringPush { dst, string, value } => {
                write!(f, "{} ← string-push({}, {})", dst, string, value)
            }
            InstrRef::IntrBytesPush { dst, bytes, value } => {
                write!(f, "{} ← bytes-push({}, {})", dst, bytes, value)
            }
            InstrRef::Pop { dst, src } => write!(f, "{} ← pop({})", dst, src),

            // Mutability
            InstrRef::Freeze { dst, src, .. } => write!(f, "{} ← freeze({})", dst, src),
            InstrRef::Thaw { dst, src, .. } => write!(f, "{} ← thaw({})", dst, src),

            // Identity
            InstrRef::Identical { dst, lhs, rhs } => {
                write!(f, "{} ← identical?({}, {})", dst, lhs, rhs)
            }
            InstrRef::CheckSignalBound { src, allowed_bits } => {
                write!(f, "check-signal-bound {} allowed={}", src, allowed_bits)
            }
        }
    }
}

// ── Terminator ──────────────────────────────────────────────────────

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Return(reg) => write!(f, "return {}", reg),
            Terminator::Jump(label) => write!(f, "jump → {}", label),
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => write!(f, "branch {} → {} / {}", cond, then_label, else_label),
            Terminator::Emit {
                signal,
                value,
                resume_label,
            } => {
                write!(f, "emit {} {} → {}", signal, value, resume_label)
            }
            Terminator::Unreachable => f.write_str("unreachable"),
        }
    }
}

/// Return the kind of a terminator as a static string suitable for use as
/// a keyword value in structured data (e.g., `:return`, `:branch`).
pub fn terminator_kind(t: &Terminator) -> &'static str {
    match t {
        Terminator::Return(_) => "return",
        Terminator::Jump(_) => "jump",
        Terminator::Branch { .. } => "branch",
        Terminator::Emit { .. } => "emit",
        Terminator::Unreachable => "unreachable",
    }
}

#[cfg(test)]
mod tests;
