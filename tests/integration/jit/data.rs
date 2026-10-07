// audited: 2026-10-06
// src/jit/AGENTS.md
// What JIT-compiled code makes of values that are not integers: floats, pairs,
// arrays, and capture cells.

use super::*;

// =============================================================================
// Float Tests
// =============================================================================

#[test]
fn test_jit_float_constant() {
    let func = function(
        Arity::Exact(0),
        1,
        Signal::silent(),
        &[(
            0,
            &[InstrRef::Const {
                dst: Reg(0),
                value: ConstRef::Float(1.234),
            }],
            Terminator::Return(Reg(0)),
        )],
    );

    let result = compile_and_call(&func, &[]).unwrap();
    assert!((result.as_float().unwrap() - 1.234).abs() < 0.001);
}

#[test]
fn test_jit_float_add() {
    let func = binary(InstrRef::binop(Reg(2), BinOp::Add, Reg(0), Reg(1)));
    let result = compile_and_call(&func, &[Value::float(1.5), Value::float(2.5)]).unwrap();
    assert!((result.as_float().unwrap() - 4.0).abs() < 0.001);
}

// =============================================================================
// Data Structure Tests
// =============================================================================

#[test]
fn test_jit_cons() {
    // fn(x, y) -> pair(x, y)
    let func = binary(InstrRef::List {
        dst: Reg(2),
        head: Reg(0),
        tail: Reg(1),
        // Real per-execution slot (>= 2).
        region: elle::hir::region::StaticRegion::new(2).unwrap(),
    });

    let result = compile_and_call(&func, &[Value::int(1), Value::int(2)]).unwrap();
    assert!(result.is_pair());
    let pair = result.as_pair().unwrap();
    assert_eq!(pair.first.as_int(), Some(1));
    assert_eq!(pair.rest.as_int(), Some(2));
}

#[test]
fn test_jit_car_cdr() {
    // fn(pair) -> first(pair) + rest(pair)
    // Assumes pair is (a . b) where a and b are integers
    let func = function(
        Arity::Exact(1),
        4,
        Signal::silent(),
        &[(
            0,
            &[
                load_arg(Reg(0), 0),
                InstrRef::First {
                    dst: Reg(1),
                    pair: Reg(0),
                },
                InstrRef::Rest {
                    dst: Reg(2),
                    pair: Reg(0),
                },
                InstrRef::binop(Reg(3), BinOp::Add, Reg(1), Reg(2)),
            ],
            Terminator::Return(Reg(3)),
        )],
    );

    // Heap args must be built in an active alloc region so the JIT can read
    // them (mirrors the runtime, where the caller has a live region when it
    // builds args and invokes the callee). Build the arg and call inside one
    // transient region; an immediate result survives its free.
    let result = {
        let h = elle::primitives::ctx::TestHeap::new();
        let pair = h.ctx().pair(Value::int(10), Value::int(32));
        compile_and_call(&func, &[pair])
    }
    .unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_is_pair() {
    // fn(x) -> is_pair(x)
    let func = unary(InstrRef::IsPair {
        dst: Reg(1),
        src: Reg(0),
    });

    // Test with a pair cell — heap arg built in a live region (see
    // test_jit_car_cdr) so the JIT can read it.
    let result = {
        let h = elle::primitives::ctx::TestHeap::new();
        let pair = h.ctx().pair(Value::int(1), Value::int(2));
        compile_and_call(&func, &[pair])
    }
    .unwrap();
    assert_eq!(result.as_bool(), Some(true));

    // Test with an integer
    let result2 = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert_eq!(result2.as_bool(), Some(false));
}

#[test]
fn test_jit_make_array() {
    // fn(a, b, c) -> array(a, b, c)
    let func = function(
        Arity::Exact(3),
        4,
        Signal::silent(),
        &[(
            0,
            &[
                load_arg(Reg(0), 0),
                load_arg(Reg(1), 1),
                load_arg(Reg(2), 2),
                InstrRef::MakeArrayMut {
                    dst: Reg(3),
                    elements: &[Reg(0), Reg(1), Reg(2)],
                    // Real per-execution slot (>= 2).
                    region: elle::hir::region::StaticRegion::new(2).unwrap(),
                },
            ],
            Terminator::Return(Reg(3)),
        )],
    );

    let result = compile_and_call(&func, &[Value::int(1), Value::int(2), Value::int(3)]).unwrap();
    assert!(result.is_array_mut());
    let vec = result.as_array_mut().unwrap();
    let borrowed = vec.borrow();
    assert_eq!(borrowed.len(), 3);
    assert_eq!(borrowed[0].as_int(), Some(1));
    assert_eq!(borrowed[1].as_int(), Some(2));
    assert_eq!(borrowed[2].as_int(), Some(3));
}

// =============================================================================
// Cell Tests
// =============================================================================

#[test]
fn test_jit_make_lbox() {
    // fn(x) -> make_lbox(x)
    let func = unary(InstrRef::MakeCaptureCell {
        dst: Reg(1),
        value: Reg(0),
        // Real per-execution slot (>= 2).
        region: elle::hir::region::StaticRegion::new(2).unwrap(),
        name: elle::value::SymbolId::of("jit-cell"),
        mutated: true,
    });

    let result = compile_and_call(&func, &[Value::int(42)]).unwrap();
    assert!(result.is_capture_cell());
    let cell = result.as_capture_cell().unwrap();
    assert_eq!(cell.borrow().as_int(), Some(42));
}

#[test]
fn test_jit_load_lbox() {
    // fn(cell) -> load_lbox(cell)
    let func = unary(InstrRef::LoadCaptureCell {
        dst: Reg(1),
        cell: Reg(0),
    });

    // Heap arg (capture cell) built in a live region (see test_jit_car_cdr).
    let result = {
        let h = elle::primitives::ctx::TestHeap::new();
        let cell = h
            .ctx()
            .capture_cell(Value::int(42), elle::value::heap::CellOrigin::Runtime);
        compile_and_call(&func, &[cell])
    }
    .unwrap();
    assert_eq!(result.as_int(), Some(42));
}

#[test]
fn test_jit_store_lbox() {
    // fn(cell, value) -> store_lbox(cell, value); load_lbox(cell)
    let func = function(
        Arity::Exact(2),
        3,
        Signal::silent(),
        &[(
            0,
            &[
                load_arg(Reg(0), 0), // cell
                load_arg(Reg(1), 1), // value
                InstrRef::StoreCaptureCell {
                    cell: Reg(0),
                    value: Reg(1),
                },
                InstrRef::LoadCaptureCell {
                    dst: Reg(2),
                    cell: Reg(0),
                },
            ],
            Terminator::Return(Reg(2)),
        )],
    );

    // Heap arg (capture cell) built in a live region (see test_jit_car_cdr).
    let result = {
        let h = elle::primitives::ctx::TestHeap::new();
        let cell = h
            .ctx()
            .capture_cell(Value::int(0), elle::value::heap::CellOrigin::Runtime);
        compile_and_call(&func, &[cell, Value::int(42)])
    }
    .unwrap();
    assert_eq!(result.as_int(), Some(42));
}
