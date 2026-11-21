// audited: 2026-11-21
//! The data form names every opcode's kind, and every field of its variant, in kebab case.
//!
//! docs/spec/form.md
//!
//! `exemplar` builds one instruction per opcode, so the field test reaches
//! every variant. The field names it expects come from each variant's `Debug`
//! text, which Rust derives from the declaration: a form that drops a field,
//! reorders two, or keeps an underscore fails against it.

use super::*;
use crate::lir::code::form::{self, Field};
use crate::lir::Label;
use std::collections::HashSet;

/// The field names a derived `Debug` spells for `instr`, in declaration order.
fn debug_fields(instr: &InstrRef<'_>) -> Vec<String> {
    let text = format!("{:?}", instr);
    let Some(open) = text.find(" { ") else {
        return Vec::new();
    };
    let body = &text[open + 3..text.len() - 2];
    let mut names = Vec::new();
    let (mut depth, mut quoted, mut start) = (0i32, false, 0usize);
    let bytes = body.as_bytes();
    for (i, &c) in bytes.iter().enumerate() {
        match c {
            b'"' => quoted = !quoted,
            b'{' | b'[' | b'(' if !quoted => depth += 1,
            b'}' | b']' | b')' if !quoted => depth -= 1,
            b',' if !quoted && depth == 0 => {
                names.push(body[start..i].trim().to_string());
                start = i + 1;
            }
            _ => {}
        }
    }
    names.push(body[start..].trim().to_string());
    names
        .into_iter()
        .map(|part| part.split(':').next().unwrap_or_default().to_string())
        .collect()
}

#[test]
fn every_opcode_names_its_kind_in_kebab_case() {
    assert_eq!(form::kind_name(Op::Const), "const");
    assert_eq!(form::kind_name(Op::LoadLocal), "load-local");
    assert_eq!(form::kind_name(Op::BinOp), "bin-op");
    assert_eq!(form::kind_name(Op::IsSymbolCheck), "is-symbol-check");
    assert_eq!(
        form::kind_name(Op::ArrayMutRefOrNil),
        "array-mut-ref-or-nil"
    );
    assert_eq!(form::kind_name(Op::TailCallArrayMut), "tail-call-array-mut");
    let names: HashSet<String> = Op::ALL.iter().map(|&op| form::kind_name(op)).collect();
    assert_eq!(names.len(), Op::ALL.len(), "two opcodes share a kind");
    for name in &names {
        assert!(
            name.chars().all(|c| c.is_ascii_lowercase() || c == '-'),
            "{name} is not kebab case"
        );
    }
}

#[test]
fn every_field_of_every_variant_is_a_key_in_order() {
    for &op in Op::ALL {
        let instr = exemplar(op);
        let form = form::instr(&instr);
        assert_eq!(form.kind, form::kind_name(op), "{op:?}");
        let names: Vec<String> = form.fields.iter().map(|(n, _)| n.to_string()).collect();
        let expected: Vec<String> = debug_fields(&instr)
            .iter()
            .map(|n| n.replace('_', "-"))
            .collect();
        assert_eq!(names, expected, "{op:?}");
    }
}

#[test]
fn each_kind_of_field_holds_its_value() {
    let fields = |op| form::instr(&exemplar(op)).fields;
    assert_eq!(
        fields(Op::Call),
        vec![
            ("dst", Field::Int(1)),
            ("func", Field::Int(2)),
            ("args", Field::Ints(vec![3, 4, 5])),
            ("arity-checked", Field::Bool(true)),
            ("region", Field::Int(23)),
            ("moves", Field::Offers(vec![(0, 6), (2, 300)])),
        ]
    );
    assert_eq!(
        fields(Op::TailCall)[5..],
        [
            ("defer-callee-release", Field::Bool(true)),
            ("deferred-release-slot", Field::Int(31)),
            ("borrowed-arg-slots", Field::Ints(vec![7, 9])),
            ("late-mints", Field::Offers(vec![(1, 9)])),
        ]
    );
    assert_eq!(fields(Op::BinOp)[1], ("op", Field::Keyword("shr")));
    assert_eq!(fields(Op::BinOp)[4], ("proof", Field::Keyword("int")));
    assert_eq!(fields(Op::UnaryOp)[1], ("op", Field::Keyword("bit-not")));
    assert_eq!(
        fields(Op::Convert)[1],
        ("op", Field::Keyword("float-to-int"))
    );
    assert_eq!(fields(Op::Compare)[1], ("op", Field::Keyword("ge")));
    assert_eq!(
        fields(Op::Const)[1],
        ("value", Field::Const(ConstRef::Int(-7)))
    );
    assert_eq!(
        fields(Op::ValueConst)[1],
        ("value", Field::Value(Value::int(77)))
    );
    let InstrRef::MaterializeConst { template, .. } = exemplar(Op::MaterializeConst) else {
        unreachable!()
    };
    assert_eq!(
        fields(Op::MaterializeConst)[1],
        ("template", Field::Template(template))
    );
    assert_eq!(
        fields(Op::StructRest)[2],
        (
            "exclude-keys",
            Field::Consts(vec![
                ConstRef::Keyword(0xbeef),
                ConstRef::Symbol(SymbolId::of("b-key")),
            ])
        )
    );
    assert_eq!(
        fields(Op::MakeCaptureCell)[3..],
        [
            ("name", Field::Symbol(SymbolId::of("a-cell"))),
            ("mutated", Field::Bool(true)),
        ]
    );
    assert_eq!(
        fields(Op::CheckSignalBound)[1],
        (
            "allowed-bits",
            Field::Signal(SignalBits::new(0x8000_0000_0000_0011))
        )
    );
    assert_eq!(
        fields(Op::MakeClosure)[1..3],
        [
            ("closure-id", Field::Int(4)),
            ("captures", Field::Ints(vec![2, 3, 4])),
        ]
    );
}

#[test]
fn an_absent_slot_is_absent() {
    let InstrRef::TailCall {
        dst,
        func,
        args,
        arity_checked,
        region,
        defer_callee_release,
        borrowed_arg_slots,
        late_mints,
        ..
    } = exemplar(Op::TailCall)
    else {
        unreachable!()
    };
    let call = InstrRef::TailCall {
        dst,
        func,
        args,
        arity_checked,
        region,
        defer_callee_release,
        deferred_release_slot: None,
        borrowed_arg_slots,
        late_mints,
    };
    assert_eq!(
        form::instr(&call).fields[6],
        ("deferred-release-slot", Field::Absent)
    );
}

#[test]
fn each_terminator_names_its_fields() {
    let t = form::terminator(&Terminator::Return(Reg(4)));
    assert_eq!(
        (t.kind.as_str(), t.fields),
        ("return", vec![("value", Field::Int(4))])
    );
    let t = form::terminator(&Terminator::Jump(Label(3)));
    assert_eq!(
        (t.kind.as_str(), t.fields),
        ("jump", vec![("target", Field::Int(3))])
    );
    let t = form::terminator(&Terminator::Branch {
        cond: Reg(1),
        then_label: Label(2),
        else_label: Label(3),
    });
    assert_eq!(
        (t.kind.as_str(), t.fields),
        (
            "branch",
            vec![
                ("cond", Field::Int(1)),
                ("then-label", Field::Int(2)),
                ("else-label", Field::Int(3)),
            ]
        )
    );
    let bits = SignalBits::new(0x2);
    let t = form::terminator(&Terminator::Emit {
        signal: bits,
        value: Reg(5),
        resume_label: Label(6),
    });
    assert_eq!(
        (t.kind.as_str(), t.fields),
        (
            "emit",
            vec![
                ("signal", Field::Signal(bits)),
                ("value", Field::Int(5)),
                ("resume-label", Field::Int(6)),
            ]
        )
    );
    let t = form::terminator(&Terminator::Unreachable);
    assert_eq!((t.kind.as_str(), t.fields), ("unreachable", vec![]));
}
