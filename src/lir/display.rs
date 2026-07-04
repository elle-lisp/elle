//! Compact human-readable display for LIR instructions and terminators.
//!
//! The Debug format is verbose Rust struct syntax. This module provides
//! a compact format designed for CFG visualization:
//!   `Const { dst: Reg(0), value: Int(42) }` → `r0 ← 42`
//!   `BinOp { dst: Reg(2), op: Add, lhs: Reg(0), rhs: Reg(1) }` → `r2 ← r0 + r1`

use super::types::*;
use std::fmt;

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

// ── LirConst ────────────────────────────────────────────────────────

impl fmt::Display for LirConst {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            LirConst::Nil => f.write_str("nil"),
            LirConst::EmptyList => f.write_str("()"),
            LirConst::Bool(true) => f.write_str("true"),
            LirConst::Bool(false) => f.write_str("false"),
            LirConst::Int(n) => write!(f, "{}", n),
            LirConst::Float(n) => write!(f, "{}", n),
            LirConst::String(s) => write!(f, "\"{}\"", s),
            LirConst::Symbol(sid) => write!(f, "sym({})", sid.0),
            LirConst::Keyword(k) => write!(f, ":{}", k),
            LirConst::ClosureRef(idx) => write!(f, "closure-ref({})", idx),
            LirConst::ValueRef(idx) => write!(f, "value-ref({})", idx),
        }
    }
}

// ── LirInstr ────────────────────────────────────────────────────────

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

impl fmt::Display for LirInstr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // === Constants ===
            LirInstr::Const { dst, value } => write!(f, "{} ← {}", dst, value),
            LirInstr::ValueConst { dst, value } => write!(f, "{} ← val({})", dst, value),
            LirInstr::MaterializeConst {
                dst,
                template,
                region,
            } => {
                write!(f, "{} ← materialize({:?}) @r{}", dst, template, region)
            }

            // === Variables ===
            LirInstr::LoadLocal { dst, slot } => write!(f, "{} ← local[{}]", dst, slot),
            LirInstr::StoreLocal { slot, src } => write!(f, "local[{}] ← {}", slot, src),
            LirInstr::StoreLocalRefcounted { slot, src } => {
                write!(f, "local[{}] ←rc {}", slot, src)
            }
            LirInstr::LoadCapture { dst, index } => write!(f, "{} ← cap[{}]", dst, index),
            LirInstr::LoadCaptureRaw { dst, index } => {
                write!(f, "{} ← cap[{}] (raw)", dst, index)
            }
            LirInstr::StoreCapture { index, src } => write!(f, "cap[{}] ← {}", index, src),

            // === Closures ===
            LirInstr::MakeClosure { dst, captures, .. } => {
                write!(f, "{} ← closure(", dst)?;
                fmt_regs(captures, f)?;
                f.write_str(")")
            }
            LirInstr::LoadSelf { dst } => write!(f, "{} ← self", dst),

            // === Function Calls ===
            LirInstr::Call {
                dst,
                func,
                args,
                arity_checked,
                ..
            }
            | LirInstr::SuspendingCall {
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
            LirInstr::TailCall {
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
            LirInstr::List {
                dst, head, tail, ..
            } => {
                write!(f, "{} ← pair({}, {})", dst, head, tail)
            }
            LirInstr::MakeArrayMut { dst, elements, .. } => {
                write!(f, "{} ← array(", dst)?;
                fmt_regs(elements, f)?;
                f.write_str(")")
            }
            LirInstr::First { dst, pair } => write!(f, "{} ← first({})", dst, pair),
            LirInstr::Rest { dst, pair } => write!(f, "{} ← rest({})", dst, pair),

            // === Primitive Operations ===
            LirInstr::BinOp { dst, op, lhs, rhs } => {
                write!(f, "{} ← {} {} {}", dst, lhs, op, rhs)
            }
            LirInstr::UnaryOp { dst, op, src } => write!(f, "{} ← {}{}", dst, op, src),
            LirInstr::Convert { dst, op, src } => {
                let name = match op {
                    ConvOp::IntToFloat => "float",
                    ConvOp::FloatToInt => "int",
                };
                write!(f, "{} ← {}({})", dst, name, src)
            }
            LirInstr::Compare { dst, op, lhs, rhs } => {
                write!(f, "{} ← {} {} {}", dst, lhs, op, rhs)
            }

            // === Type Checks ===
            LirInstr::IsNil { dst, src } => write!(f, "{} ← nil?({})", dst, src),
            LirInstr::IsPair { dst, src } => write!(f, "{} ← pair?({})", dst, src),
            LirInstr::IsArray { dst, src } => write!(f, "{} ← tuple?({})", dst, src),
            LirInstr::IsArrayMut { dst, src } => write!(f, "{} ← array?({})", dst, src),
            LirInstr::IsStruct { dst, src } => write!(f, "{} ← struct?({})", dst, src),
            LirInstr::IsStructMut { dst, src } => write!(f, "{} ← @struct?({})", dst, src),
            LirInstr::ArrayMutLen { dst, src } => write!(f, "{} ← len({})", dst, src),

            // === Box Operations ===
            LirInstr::MakeCaptureCell { dst, value, .. } => write!(f, "{} ← lbox({})", dst, value),
            LirInstr::LoadCaptureCell { dst, cell } => write!(f, "{} ← deref({})", dst, cell),
            LirInstr::StoreCaptureCell { cell, value } => write!(f, "deref({}) ← {}", cell, value),

            // === Destructuring ===
            LirInstr::MatchFail { dst, src } => write!(f, "{} ← match-fail!({})", dst, src),
            LirInstr::FirstDestructure { dst, src } => write!(f, "{} ← first!({})", dst, src),
            LirInstr::RestDestructure { dst, src } => write!(f, "{} ← rest!({})", dst, src),
            LirInstr::ArrayMutRefDestructure { dst, src, index } => {
                write!(f, "{} ← {}[{}]!", dst, src, index)
            }
            LirInstr::ArrayMutSliceFrom { dst, src, index } => {
                write!(f, "{} ← {}[{}..]", dst, src, index)
            }
            LirInstr::StructGetOrNil { dst, src, key } => {
                write!(f, "{} ← {}.{}?", dst, src, key)
            }
            LirInstr::StructGetDestructure { dst, src, key } => {
                write!(f, "{} ← {}.{}!", dst, src, key)
            }
            LirInstr::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                let keys: Vec<String> = exclude_keys.iter().map(|k| format!("{}", k)).collect();
                write!(f, "{} ← rest({}, excl=[{}])", dst, src, keys.join(", "))
            }

            // === Silent destructuring (parameter context) ===
            LirInstr::FirstOrNil { dst, src } => write!(f, "{} ← first?({})", dst, src),
            LirInstr::RestOrNil { dst, src } => write!(f, "{} ← rest?({})", dst, src),
            LirInstr::ArrayMutRefOrNil { dst, src, index } => {
                write!(f, "{} ← {}[{}]?", dst, src, index)
            }

            // === Fibers ===
            LirInstr::LoadResumeValue { dst } => write!(f, "{} ← resume-val", dst),

            // === Runtime Eval ===
            LirInstr::Eval { dst, expr, env } => {
                write!(f, "{} ← eval({}, {})", dst, expr, env)
            }

            // === Splice Support ===
            LirInstr::ArrayMutExtend { dst, array, source } => {
                write!(f, "{} ← extend({}, {})", dst, array, source)
            }
            LirInstr::ArrayMutPush { dst, array, value } => {
                write!(f, "{} ← push({}, {})", dst, array, value)
            }
            LirInstr::CallArrayMut {
                dst, func, args, ..
            } => {
                write!(f, "{} ← {}(;{})", dst, func, args)
            }
            LirInstr::TailCallArrayMut { func, args, .. } => {
                write!(f, "tailcall {}(;{})", func, args)
            }

            // === Allocation Regions ===
            LirInstr::IncrefRegion { region_id } => write!(f, "incref-region {region_id}"),
            LirInstr::DecrefRegion { region_id } => write!(f, "decref-region {region_id}"),
            LirInstr::DecrefValueRegion { src } => write!(f, "decref-value-region {src}"),
            LirInstr::DecrefCellRegion { src } => write!(f, "decref-cell-region {src}"),
            LirInstr::IncrefValueRegion { src } => write!(f, "incref-value-region {src}"),
            LirInstr::AdoptRegion { parent, child } => {
                write!(f, "adopt-region parent={parent} child={child}")
            }
            LirInstr::AdoptIntoActivation { child } => {
                write!(f, "adopt-into-activation {child}")
            }
            LirInstr::FreeRegionGroup { members } => {
                write!(f, "free-region-group(")?;
                fmt_regs(members, f)?;
                f.write_str(")")
            }
            LirInstr::AssertRegionMatches { region_id, src } => {
                write!(f, "assert-region-matches {region_id} {src}")
            }
            // === Dynamic Parameters ===
            LirInstr::PushParamFrame { pairs } => {
                write!(f, "push-param-frame(")?;
                for (i, (param, value)) in pairs.iter().enumerate() {
                    if i > 0 {
                        write!(f, ", ")?;
                    }
                    write!(f, "{}={}", param, value)?;
                }
                write!(f, ")")
            }
            LirInstr::PopParamFrame => f.write_str("pop-param-frame"),
            LirInstr::IsSet { dst, src } => write!(f, "{} = is-set {}", dst, src),
            LirInstr::IsSetMut { dst, src } => write!(f, "{} = is-set-mut {}", dst, src),

            // New type predicates
            LirInstr::IsEmpty { dst, src } => write!(f, "{} ← empty?({})", dst, src),
            LirInstr::IsBool { dst, src } => write!(f, "{} ← bool?({})", dst, src),
            LirInstr::IsInt { dst, src } => write!(f, "{} ← int?({})", dst, src),
            LirInstr::IsFloat { dst, src } => write!(f, "{} ← float?({})", dst, src),
            LirInstr::IsString { dst, src } => write!(f, "{} ← string?({})", dst, src),
            LirInstr::IsKeyword { dst, src } => write!(f, "{} ← keyword?({})", dst, src),
            LirInstr::IsSymbolCheck { dst, src } => write!(f, "{} ← symbol?({})", dst, src),
            LirInstr::IsBytes { dst, src } => write!(f, "{} ← bytes?({})", dst, src),
            LirInstr::IsBox { dst, src } => write!(f, "{} ← box?({})", dst, src),
            LirInstr::IsClosure { dst, src } => write!(f, "{} ← closure?({})", dst, src),
            LirInstr::IsFiber { dst, src } => write!(f, "{} ← fiber?({})", dst, src),
            LirInstr::TypeOf { dst, src } => write!(f, "{} ← type-of({})", dst, src),

            // Data access
            LirInstr::Length { dst, src } => write!(f, "{} ← length({})", dst, src),
            LirInstr::Get { dst, obj, key } => write!(f, "{} ← get({}, {})", dst, obj, key),
            LirInstr::Put { dst, obj, key, val } => {
                write!(f, "{} ← put({}, {}, {})", dst, obj, key, val)
            }
            LirInstr::Del { dst, obj, key } => write!(f, "{} ← del({}, {})", dst, obj, key),
            LirInstr::Has { dst, obj, key } => write!(f, "{} ← has?({}, {})", dst, obj, key),
            LirInstr::IntrPush { dst, array, value } => {
                write!(f, "{} ← push({}, {})", dst, array, value)
            }
            LirInstr::IntrStringPush { dst, string, value } => {
                write!(f, "{} ← string-push({}, {})", dst, string, value)
            }
            LirInstr::IntrBytesPush { dst, bytes, value } => {
                write!(f, "{} ← bytes-push({}, {})", dst, bytes, value)
            }
            LirInstr::Pop { dst, src } => write!(f, "{} ← pop({})", dst, src),

            // Mutability
            LirInstr::Freeze { dst, src, .. } => write!(f, "{} ← freeze({})", dst, src),
            LirInstr::Thaw { dst, src, .. } => write!(f, "{} ← thaw({})", dst, src),

            // Identity
            LirInstr::Identical { dst, lhs, rhs } => {
                write!(f, "{} ← identical?({}, {})", dst, lhs, rhs)
            }
            LirInstr::CheckSignalBound { src, allowed_bits } => {
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
