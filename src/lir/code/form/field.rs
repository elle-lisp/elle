// audited: 2026-11-24
//! The field each operand type is in the data form: an integer, a list of integers, a keyword, a constant, a value.
//!
//! docs/spec/form.md

use super::Field;
use crate::hir::region::StaticRegion;
use crate::lir::code::operand::{ArenaSlots, ConstList, ConstRef, Offers, Slots, TemplateBytes};
use crate::lir::{BinOp, ClosureId, CmpOp, ConvOp, Label, OperandProof, Reg, UnaryOp};
use crate::value::fiber::SignalBits;
use crate::value::{SymbolId, Value};

/// An operand type, and the field each of its values is.
pub(super) trait Operand<'a> {
    fn field(self) -> Field<'a>;
}

macro_rules! operands {
    ($($ty:ty => |$x:ident| $field:expr;)*) => {
        $(impl<'a> Operand<'a> for $ty {
            fn field(self) -> Field<'a> {
                let $x = self;
                $field
            }
        })*
    };
}

operands! {
    Reg => |r| Field::Int(r.0.into());
    Label => |l| Field::Int(l.0.into());
    ClosureId => |c| Field::Int(c.0.into());
    StaticRegion => |s| Field::Int(s.get().into());
    u16 => |n| Field::Int(n.into());
    &'a [Reg] => |rs| Field::Ints(rs.iter().map(|r| r.0.into()).collect());
    Slots<'a> => |s| Field::Ints(s.iter().map(i64::from).collect());
    ArenaSlots<'a> => |s| Field::Ints(s.iter().map(|r| r.get().into()).collect());
    ConstRef => |c| Field::Const(c);
    ConstList<'a> => |k| Field::Consts(k.iter().collect());
    Value => |v| Field::Value(v);
    TemplateBytes<'a> => |t| Field::Template(t);
    Offers<'a> => |o| Field::Offers(o.iter().map(|o| (o.position() as i64, o.slot().into())).collect());
    SignalBits => |b| Field::Signal(b);
    SymbolId => |s| Field::Symbol(s);
    bool => |b| Field::Bool(b);
    BinOp => |op| Field::Keyword(match op {
        BinOp::Add => "add",
        BinOp::Sub => "sub",
        BinOp::Mul => "mul",
        BinOp::Div => "div",
        BinOp::Rem => "rem",
        BinOp::BitAnd => "bit-and",
        BinOp::BitOr => "bit-or",
        BinOp::BitXor => "bit-xor",
        BinOp::Shl => "shl",
        BinOp::Shr => "shr",
    });
    UnaryOp => |op| Field::Keyword(match op {
        UnaryOp::Neg => "neg",
        UnaryOp::Not => "not",
        UnaryOp::BitNot => "bit-not",
    });
    ConvOp => |op| Field::Keyword(match op {
        ConvOp::IntToFloat => "int-to-float",
        ConvOp::FloatToInt => "float-to-int",
    });
    CmpOp => |op| Field::Keyword(match op {
        CmpOp::Eq => "eq",
        CmpOp::Ne => "ne",
        CmpOp::Lt => "lt",
        CmpOp::Le => "le",
        CmpOp::Gt => "gt",
        CmpOp::Ge => "ge",
    });
    OperandProof => |p| Field::Keyword(match p {
        OperandProof::Unproven => "unproven",
        OperandProof::Int => "int",
    });
}
