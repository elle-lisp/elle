// audited: 2026-11-25
//! The data form of one instruction or terminator: its kind, and each field of its variant by its kebab-case name.
//!
//! docs/spec/form.md
//!
//! Each operand type has one form, which the `Operand` impls in
//! form/field.rs give, as the document's table does. `forms!` lists every
//! variant's fields once, and expands to one match with no wildcard and no
//! `..`, so a new variant or a new field does not compile until it is listed
//! here.

use super::instr::InstrRef;
use super::op::Op;
use super::operand::{ConstRef, TemplateBytes};
use crate::lir::Terminator;
use crate::value::fiber::SignalBits;
use crate::value::{SymbolId, Value};
use field::Operand;

mod field;

/// One field's value, before it becomes an Elle value.
#[derive(Clone, Debug, PartialEq)]
pub enum Field<'a> {
    /// A register, a label, a slot, an index, a static region or a closure id.
    Int(i64),
    /// A list of registers, of slots or of static regions.
    Ints(Vec<i64>),
    Const(ConstRef),
    Consts(Vec<ConstRef>),
    Value(Value),
    Template(TemplateBytes<'a>),
    /// Each offer as `(position, slot)`.
    Offers(Vec<(i64, i64)>),
    /// An operator or an operand proof, spelled without its colon.
    Keyword(&'static str),
    Signal(SignalBits),
    /// The name of a binding, as a capture cell's.
    Symbol(SymbolId),
    Bool(bool),
}

/// An instruction or a terminator in the data form.
#[derive(Clone, Debug, PartialEq)]
pub struct Form<'a> {
    /// The variant's name in kebab case.
    pub kind: String,
    /// Each field of the variant, in declaration order.
    pub fields: Vec<(&'static str, Field<'a>)>,
}

/// `op`'s name in kebab case: `LoadLocal` is `load-local`.
pub fn kind_name(op: Op) -> String {
    let mut kind = String::new();
    for (i, c) in format!("{op:?}").chars().enumerate() {
        if i > 0 && c.is_ascii_uppercase() {
            kind.push('-');
        }
        kind.push(c.to_ascii_lowercase());
    }
    kind
}

/// From one arm's field list: the pattern for `$variant`, or the fields the
/// pattern binds. A field is named as it is declared, or by the kebab-case
/// name written beside a field whose name has an underscore.
macro_rules! arm {
    ($variant:ident { $($field:ident $(= $name:literal)?),* }) => {
        InstrRef::$variant { $($field),* }
    };
    ({ $($field:ident $(= $name:literal)?),* }) => {
        vec![$((arm!(@name $field $($name)?), $field.field())),*]
    };
    (@name $field:ident) => {
        stringify!($field)
    };
    (@name $field:ident $name:literal) => {
        $name
    };
}

/// The match over `$instr` whose arm for each variant lists its fields.
macro_rules! forms {
    ($instr:expr; $($($variant:ident)|+ => $fields:tt)*) => {
        match $instr {
            $($(arm!($variant $fields))|+ => arm!($fields),)*
        }
    };
}

/// The form of `instr`.
pub fn instr<'a>(instr: &InstrRef<'a>) -> Form<'a> {
    let fields = forms! { *instr;
        Const => { dst, value }
        ValueConst => { dst, value }
        MaterializeConst => { dst, template, region }
        LoadLocal => { dst, slot }
        StoreLocal => { slot, src }
        LoadCapture | LoadCaptureRaw => { dst, index }
        StoreCapture => { index, src }
        MakeClosure => { dst, closure_id = "closure-id", captures, region }
        LoadSelf | LoadResumeValue => { dst }
        Call | SuspendingCall => {
            dst, func, args, arity_checked = "arity-checked", region, moves
        }
        TailCall => {
            dst, func, args, arity_checked = "arity-checked", region,
            defer_callee_release = "defer-callee-release",
            deferred_release_slots = "deferred-release-slots",
            borrowed_arg_slots = "borrowed-arg-slots", late_mints = "late-mints"
        }
        List => { dst, head, tail, region }
        MakeArrayMut => { dst, elements, region }
        First | Rest => { dst, pair }
        BinOp => { dst, op, lhs, rhs, proof }
        Compare => { dst, op, lhs, rhs, proof }
        UnaryOp => { dst, op, src, proof }
        Convert => { dst, op, src }
        IsNil | IsPair | IsArray | IsArrayMut | IsStruct | IsStructMut | IsSet | IsSetMut
        | SyntaxView | ArrayMutLen | MatchFail | FirstDestructure | RestDestructure | FirstOrNil
        | RestOrNil | IsEmpty | IsBool | IsInt | IsFloat | IsString | IsKeyword
        | IsSymbolCheck | IsBytes | IsBox | IsClosure | IsFiber | TypeOf | Length
        | Pop => { dst, src }
        MakeCaptureCell => { dst, value, region, name, mutated }
        LoadCaptureCell => { dst, cell }
        StoreCaptureCell => { cell, value }
        ArrayMutRefDestructure | ArraySliceFrom | ArrayMutSliceFrom | ArrayMutRefOrNil => {
            dst, src, index
        }
        StructGetOrNil | StructHasKey | StructGetDestructure => { dst, src, key }
        StructRest => { dst, src, exclude_keys = "exclude-keys" }
        Eval => { dst, expr, env }
        ArrayMutExtend => { dst, array, source }
        ArrayMutPush | IntrPush => { dst, array, value }
        CallArrayMut => { dst, func, args, region, args_region = "args-region" }
        TailCallArrayMut => { func, args, region, args_region = "args-region" }
        IncrefRegion | DecrefRegion => { region_id = "region-id" }
        DecrefValueRegion | DecrefCellRegion | IncrefValueRegion => { src }
        DecrefPostponed => { slot, route }
        AdoptRegion | AdoptCellRegion => { parent, child }
        FreeRegionGroup => { members }
        AdoptIntoActivation => { child }
        AssertRegionMatches => { region_id = "region-id", src }
        PushParamFrame => { pairs }
        PopParamFrame => {}
        CheckSignalBound => { src, allowed_bits = "allowed-bits" }
        Get | Del | Has => { dst, obj, key }
        Put => { dst, obj, key, val }
        IntrStringPush => { dst, string, value }
        IntrBytesPush => { dst, bytes, value }
        Freeze | Thaw => { dst, src, region }
        Identical => { dst, lhs, rhs }
    };
    Form {
        kind: kind_name(Op::of(instr)),
        fields,
    }
}

/// The form of `term`.
pub fn terminator(term: &Terminator) -> Form<'static> {
    let (kind, fields) = match *term {
        Terminator::Return(value) => ("return", vec![("value", value.field())]),
        Terminator::Jump(target) => ("jump", vec![("target", target.field())]),
        Terminator::Branch {
            cond,
            then_label,
            else_label,
        } => (
            "branch",
            vec![
                ("cond", cond.field()),
                ("then-label", then_label.field()),
                ("else-label", else_label.field()),
            ],
        ),
        Terminator::Emit {
            signal,
            value,
            resume_label,
        } => (
            "emit",
            vec![
                ("signal", signal.field()),
                ("value", value.field()),
                ("resume-label", resume_label.field()),
            ],
        ),
        Terminator::Unreachable => ("unreachable", vec![]),
    };
    Form {
        kind: kind.to_string(),
        fields,
    }
}
