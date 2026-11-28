// audited: 2026-11-25
//! Compact human-readable display for LIR instructions and terminators.
//!
//! src/lir/overview.md
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
            ConstRef::Int(n) => write!(f, "{n}"),
            ConstRef::Float(n) => write!(f, "{n}"),
            ConstRef::Symbol(sid) => write!(f, "sym({})", sid.0),
            ConstRef::Keyword(hash) => write!(f, "kw({hash:#x})"),
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
        write!(f, "{r}")?;
    }
    Ok(())
}

/// A call's offers as `label[position→local[slot] …]`, or nothing for a call
/// that offers none.
fn fmt_offers(
    label: &str,
    offers: &super::code::Offers<'_>,
    f: &mut fmt::Formatter<'_>,
) -> fmt::Result {
    if offers.is_empty() {
        return Ok(());
    }
    write!(f, "{label}[")?;
    for (i, o) in offers.iter().enumerate() {
        if i > 0 {
            f.write_str(" ")?;
        }
        write!(f, "{}→local[{}]", o.position(), o.slot())?;
    }
    f.write_str("]")
}

impl fmt::Display for InstrRef<'_> {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            // === Constants ===
            InstrRef::Const { dst, value } => write!(f, "{dst} ← {value}"),
            InstrRef::ValueConst { dst, value } => write!(f, "{dst} ← val({value})"),
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
            InstrRef::LoadLocal { dst, slot } => write!(f, "{dst} ← local[{slot}]"),
            InstrRef::StoreLocal { slot, src } => write!(f, "local[{slot}] ← {src}"),
            InstrRef::LoadCapture { dst, index } => write!(f, "{dst} ← cap[{index}]"),
            InstrRef::LoadCaptureRaw { dst, index } => write!(f, "{dst} ← cap[{index}] (raw)"),
            InstrRef::StoreCapture { index, src } => write!(f, "cap[{index}] ← {src}"),

            // === Closures ===
            InstrRef::MakeClosure { dst, captures, .. } => {
                write!(f, "{dst} ← closure(")?;
                fmt_regs(captures, f)?;
                f.write_str(")")
            }
            InstrRef::LoadSelf { dst } => write!(f, "{dst} ← self"),

            // === Function Calls ===
            InstrRef::Call {
                dst,
                func,
                args,
                arity_checked,
                moves,
                ..
            }
            | InstrRef::SuspendingCall {
                dst,
                func,
                args,
                arity_checked,
                moves,
                ..
            } => {
                write!(f, "{dst} ← {func}(")?;
                fmt_regs(args, f)?;
                f.write_str(if *arity_checked { ") ✓" } else { ")" })?;
                fmt_offers(" moves", moves, f)
            }
            InstrRef::TailCall {
                func,
                args,
                arity_checked,
                late_mints,
                ..
            } => {
                write!(f, "tailcall {func}(")?;
                fmt_regs(args, f)?;
                f.write_str(if *arity_checked { ") ✓" } else { ")" })?;
                fmt_offers(" late", late_mints, f)
            }

            // === Data Construction ===
            InstrRef::List {
                dst, head, tail, ..
            } => write!(f, "{dst} ← pair({head}, {tail})"),
            InstrRef::MakeArrayMut { dst, elements, .. } => {
                write!(f, "{dst} ← array(")?;
                fmt_regs(elements, f)?;
                f.write_str(")")
            }
            InstrRef::First { dst, pair } => write!(f, "{dst} ← first({pair})"),
            InstrRef::Rest { dst, pair } => write!(f, "{dst} ← rest({pair})"),

            // === Primitive Operations ===
            InstrRef::BinOp {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => write!(f, "{} ← {} {} {}{}", dst, lhs, op, rhs, proof_mark(*proof)),
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
                write!(f, "{dst} ← {name}({src})")
            }
            InstrRef::Compare {
                dst,
                op,
                lhs,
                rhs,
                proof,
            } => write!(f, "{} ← {} {} {}{}", dst, lhs, op, rhs, proof_mark(*proof)),

            // === Type Checks ===
            InstrRef::IsNil { dst, src } => write!(f, "{dst} ← nil?({src})"),
            InstrRef::IsPair { dst, src } => write!(f, "{dst} ← pair?({src})"),
            InstrRef::IsArray { dst, src } => write!(f, "{dst} ← array?({src})"),
            InstrRef::IsArrayMut { dst, src } => write!(f, "{dst} ← @array?({src})"),
            InstrRef::IsStruct { dst, src } => write!(f, "{dst} ← struct?({src})"),
            InstrRef::IsStructMut { dst, src } => write!(f, "{dst} ← @struct?({src})"),
            InstrRef::IsSet { dst, src } => write!(f, "{dst} ← set?({src})"),
            InstrRef::IsSetMut { dst, src } => write!(f, "{dst} ← @set?({src})"),
            InstrRef::SyntaxView { dst, src } => write!(f, "{dst} ← view({src})"),
            InstrRef::IsEmpty { dst, src } => write!(f, "{dst} ← empty?({src})"),
            InstrRef::IsBool { dst, src } => write!(f, "{dst} ← bool?({src})"),
            InstrRef::IsInt { dst, src } => write!(f, "{dst} ← int?({src})"),
            InstrRef::IsFloat { dst, src } => write!(f, "{dst} ← float?({src})"),
            InstrRef::IsString { dst, src } => write!(f, "{dst} ← string?({src})"),
            InstrRef::IsKeyword { dst, src } => write!(f, "{dst} ← keyword?({src})"),
            InstrRef::IsSymbolCheck { dst, src } => write!(f, "{dst} ← symbol?({src})"),
            InstrRef::IsBytes { dst, src } => write!(f, "{dst} ← bytes?({src})"),
            InstrRef::IsBox { dst, src } => write!(f, "{dst} ← box?({src})"),
            InstrRef::IsClosure { dst, src } => write!(f, "{dst} ← closure?({src})"),
            InstrRef::IsFiber { dst, src } => write!(f, "{dst} ← fiber?({src})"),
            InstrRef::TypeOf { dst, src } => write!(f, "{dst} ← type-of({src})"),
            InstrRef::ArrayMutLen { dst, src } => write!(f, "{dst} ← len({src})"),

            // === Capture Cells ===
            InstrRef::MakeCaptureCell { dst, value, .. } => write!(f, "{dst} ← cell({value})"),
            InstrRef::LoadCaptureCell { dst, cell } => write!(f, "{dst} ← deref({cell})"),
            InstrRef::StoreCaptureCell { cell, value } => write!(f, "deref({cell}) ← {value}"),

            // === Destructuring ===
            InstrRef::MatchFail { dst, src } => write!(f, "{dst} ← match-fail!({src})"),
            InstrRef::FirstDestructure { dst, src } => write!(f, "{dst} ← first!({src})"),
            InstrRef::RestDestructure { dst, src } => write!(f, "{dst} ← rest!({src})"),
            InstrRef::ArrayMutRefDestructure { dst, src, index } => {
                write!(f, "{dst} ← {src}[{index}]!")
            }
            InstrRef::ArraySliceFrom { dst, src, index } => {
                write!(f, "{dst} ← {src}[{index}..]")
            }
            InstrRef::ArrayMutSliceFrom { dst, src, index } => {
                write!(f, "{dst} ← @{src}[{index}..]")
            }
            InstrRef::StructGetOrNil { dst, src, key } => write!(f, "{dst} ← {src}.{key}?"),
            InstrRef::StructHasKey { dst, src, key } => write!(f, "{dst} ← has-key?({src}, {key})"),
            InstrRef::StructGetDestructure { dst, src, key } => write!(f, "{dst} ← {src}.{key}!"),
            InstrRef::StructRest {
                dst,
                src,
                exclude_keys,
            } => {
                let keys: Vec<String> = exclude_keys.iter().map(|k| k.to_string()).collect();
                write!(f, "{dst} ← rest({src}, excl=[{}])", keys.join(", "))
            }

            // === Silent destructuring (parameter context) ===
            InstrRef::FirstOrNil { dst, src } => write!(f, "{dst} ← first?({src})"),
            InstrRef::RestOrNil { dst, src } => write!(f, "{dst} ← rest?({src})"),
            InstrRef::ArrayMutRefOrNil { dst, src, index } => write!(f, "{dst} ← {src}[{index}]?"),

            // === Fibers ===
            InstrRef::LoadResumeValue { dst } => write!(f, "{dst} ← resume-val"),

            // === Runtime Eval ===
            InstrRef::Eval { dst, expr, env } => write!(f, "{dst} ← eval({expr}, {env})"),

            // === Splice Support ===
            InstrRef::ArrayMutExtend { dst, array, source } => {
                write!(f, "{dst} ← extend({array}, {source})")
            }
            InstrRef::ArrayMutPush { dst, array, value } => {
                write!(f, "{dst} ← push({array}, {value})")
            }
            InstrRef::CallArrayMut {
                dst,
                func,
                args,
                args_region,
                ..
            } => {
                write!(f, "{dst} ← {func}(;{args}) [args-region {args_region}]")
            }
            InstrRef::TailCallArrayMut {
                func,
                args,
                args_region,
                ..
            } => {
                write!(f, "tailcall {func}(;{args}) [args-region {args_region}]")
            }

            // === Allocation Regions ===
            InstrRef::IncrefRegion { region_id } => write!(f, "incref-region {region_id}"),
            InstrRef::DecrefRegion { region_id } => write!(f, "decref-region {region_id}"),
            InstrRef::DecrefValueRegion { src } => write!(f, "decref-value-region {src}"),
            InstrRef::DecrefCellRegion { src } => write!(f, "decref-cell-region {src}"),
            InstrRef::DecrefPostponed { slot, route } => {
                write!(f, "decref-postponed local[{slot}] route=local[{route}]")
            }
            InstrRef::IncrefValueRegion { src } => write!(f, "incref-value-region {src}"),
            InstrRef::AdoptRegion { parent, child } => {
                write!(f, "adopt-region parent={parent} child={child}")
            }
            InstrRef::AdoptCellRegion { parent, child } => {
                write!(f, "adopt-cell-region parent={parent} child={child}")
            }
            InstrRef::AdoptIntoActivation { child } => write!(f, "adopt-into-activation {child}"),
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

            // === Data Access ===
            InstrRef::Length { dst, src } => write!(f, "{dst} ← length({src})"),
            InstrRef::Get { dst, obj, key } => write!(f, "{dst} ← get({obj}, {key})"),
            InstrRef::Put { dst, obj, key, val } => write!(f, "{dst} ← put({obj}, {key}, {val})"),
            InstrRef::Del { dst, obj, key } => write!(f, "{dst} ← del({obj}, {key})"),
            InstrRef::Has { dst, obj, key } => write!(f, "{dst} ← has?({obj}, {key})"),
            InstrRef::IntrPush { dst, array, value } => write!(f, "{dst} ← push({array}, {value})"),
            InstrRef::IntrStringPush { dst, string, value } => {
                write!(f, "{dst} ← string-push({string}, {value})")
            }
            InstrRef::IntrBytesPush { dst, bytes, value } => {
                write!(f, "{dst} ← bytes-push({bytes}, {value})")
            }
            InstrRef::Pop { dst, src } => write!(f, "{dst} ← pop({src})"),

            // === Mutability ===
            InstrRef::Freeze { dst, src, .. } => write!(f, "{dst} ← freeze({src})"),
            InstrRef::Thaw { dst, src, .. } => write!(f, "{dst} ← thaw({src})"),

            // === Identity ===
            InstrRef::Identical { dst, lhs, rhs } => write!(f, "{dst} ← identical?({lhs}, {rhs})"),
            InstrRef::CheckSignalBound { src, allowed_bits } => {
                write!(f, "check-signal-bound {src} allowed={allowed_bits}")
            }
        }
    }
}

// ── Terminator ──────────────────────────────────────────────────────

impl fmt::Display for Terminator {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Terminator::Return(reg) => write!(f, "return {reg}"),
            Terminator::Jump(label) => write!(f, "jump → {label}"),
            Terminator::Branch {
                cond,
                then_label,
                else_label,
            } => write!(f, "branch {cond} → {then_label} / {else_label}"),
            Terminator::Emit {
                signal,
                value,
                resume_label,
            } => write!(f, "emit {signal} {value} → {resume_label}"),
            Terminator::Unreachable => f.write_str("unreachable"),
        }
    }
}

#[cfg(test)]
mod tests;
