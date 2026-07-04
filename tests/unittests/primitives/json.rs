use super::*;

// JSON Parsing and Serialization Tests
#[test]
fn test_json_parse_null() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("null")]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::NIL);
}

#[test]
fn test_json_parse_booleans() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("true")]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::bool(true));

    let result = call_primitive(&json_parse, &[h.ctx().string("false")]);
    assert!(result.is_ok());
    assert_eq!(result.unwrap(), Value::bool(false));
}

#[test]
fn test_json_parse_integers() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("0")]);
    assert_eq!(result.unwrap(), Value::int(0));

    let result = call_primitive(&json_parse, &[h.ctx().string("42")]);
    assert_eq!(result.unwrap(), Value::int(42));

    let result = call_primitive(&json_parse, &[h.ctx().string("-17")]);
    assert_eq!(result.unwrap(), Value::int(-17));
}

#[test]
#[allow(clippy::approx_constant)]
fn test_json_parse_floats() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("3.14")]);
    if let Some(f) = result.unwrap().as_float() {
        assert!((f - 3.14).abs() < 1e-10)
    } else {
        panic!("Expected float");
    }

    let result = call_primitive(&json_parse, &[h.ctx().string("1e10")]);
    if let Some(f) = result.unwrap().as_float() {
        assert!((f - 1e10).abs() < 1e5)
    } else {
        panic!("Expected float");
    }

    let result = call_primitive(&json_parse, &[h.ctx().string("1.0")]);
    if let Some(f) = result.unwrap().as_float() {
        assert!((f - 1.0).abs() < 1e-10)
    } else {
        panic!("Expected float");
    }
}

#[test]
fn test_json_parse_strings() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("\"hello\"")]);
    assert_eq!(result.unwrap(), h.ctx().string("hello"));

    let result = call_primitive(&json_parse, &[h.ctx().string("\"\"")]);
    assert_eq!(result.unwrap(), h.ctx().string(""));

    let result = call_primitive(&json_parse, &[h.ctx().string("\"hello\\nworld\"")]);
    assert_eq!(result.unwrap(), h.ctx().string("hello\nworld"));

    let result = call_primitive(&json_parse, &[h.ctx().string("\"quote\\\"test\"")]);
    assert_eq!(result.unwrap(), h.ctx().string("quote\"test"));

    let result = call_primitive(&json_parse, &[h.ctx().string("\"\\u0041\"")]);
    assert_eq!(result.unwrap(), h.ctx().string("A"));
}

#[test]
fn test_json_parse_arrays() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("[]")]);
    assert_eq!(result.unwrap(), Value::EMPTY_LIST);

    let result = call_primitive(&json_parse, &[h.ctx().string("[1,2,3]")]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], Value::int(2));
    assert_eq!(vec[2], Value::int(3));

    let result = call_primitive(&json_parse, &[h.ctx().string("[1,\"two\",true,null]")]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 4);
    assert_eq!(vec[0], Value::int(1));
    assert_eq!(vec[1], h.ctx().string("two"));
    assert_eq!(vec[2], Value::bool(true));
    assert_eq!(vec[3], Value::NIL);
}

#[test]
fn test_json_parse_objects() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("{}")]);
    match result.unwrap() {
        v if v.as_struct_mut().is_some() => {
            let t = v.as_struct_mut().unwrap();
            assert_eq!(t.borrow().len(), 0);
        }
        _ => panic!("Expected table"),
    }

    let result = call_primitive(
        &json_parse,
        &[h.ctx().string("{\"name\":\"Alice\",\"age\":30}")],
    );
    match result.unwrap() {
        v if v.as_struct_mut().is_some() => {
            let t = v.as_struct_mut().unwrap();
            let table = t.borrow();
            assert_eq!(table.len(), 2);
        }
        _ => panic!("Expected table"),
    }
}

#[test]
fn test_json_parse_whitespace() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("  \n\t  42  \n\t  ")]);
    assert_eq!(result.unwrap(), Value::int(42));

    let result = call_primitive(&json_parse, &[h.ctx().string("[ 1 , 2 , 3 ]")]);
    let list = result.unwrap();
    let vec = list.list_to_vec().unwrap();
    assert_eq!(vec.len(), 3);
}

#[test]
fn test_json_parse_errors() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    // Empty input
    let result = call_primitive(&json_parse, &[h.ctx().string("")]);
    assert!(result.is_err());

    // Trailing content
    let result = call_primitive(&json_parse, &[h.ctx().string("42 extra")]);
    assert!(result.is_err());

    // Unterminated string
    let result = call_primitive(&json_parse, &[h.ctx().string("\"unterminated")]);
    assert!(result.is_err());

    // Unclosed array
    let result = call_primitive(&json_parse, &[h.ctx().string("[1,2")]);
    assert!(result.is_err());

    // Unclosed object
    let result = call_primitive(&json_parse, &[h.ctx().string("{\"key\":42")]);
    assert!(result.is_err());

    // Invalid token
    let result = call_primitive(&json_parse, &[h.ctx().string("invalid")]);
    assert!(result.is_err());
}

#[test]
fn test_json_serialize_compact() {
    let (_vm, mut symbols, meta) = setup();
    let json_serialize = get_primitive(&meta, &mut symbols, "json-serialize");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_serialize, &[Value::NIL]);
    assert_eq!(result.unwrap(), h.ctx().string("null"));

    let result = call_primitive(&json_serialize, &[Value::bool(true)]);
    assert_eq!(result.unwrap(), h.ctx().string("true"));

    let result = call_primitive(&json_serialize, &[Value::bool(false)]);
    assert_eq!(result.unwrap(), h.ctx().string("false"));

    let result = call_primitive(&json_serialize, &[Value::int(42)]);
    assert_eq!(result.unwrap(), h.ctx().string("42"));

    let result = call_primitive(&json_serialize, &[h.ctx().string("hello")]);
    assert_eq!(result.unwrap(), h.ctx().string("\"hello\""));

    let list = h
        .ctx()
        .list(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let result = call_primitive(&json_serialize, &[list]);
    assert_eq!(result.unwrap(), h.ctx().string("[1,2,3]"));
}

#[test]
fn test_json_serialize_string_escaping() {
    let (_vm, mut symbols, meta) = setup();
    let json_serialize = get_primitive(&meta, &mut symbols, "json-serialize");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_serialize, &[h.ctx().string("hello\"world")]);
    assert_eq!(result.unwrap(), h.ctx().string("\"hello\\\"world\""));

    let result = call_primitive(&json_serialize, &[h.ctx().string("hello\\world")]);
    assert_eq!(result.unwrap(), h.ctx().string("\"hello\\\\world\""));

    let result = call_primitive(&json_serialize, &[h.ctx().string("hello\nworld")]);
    assert_eq!(result.unwrap(), h.ctx().string("\"hello\\nworld\""));

    let result = call_primitive(&json_serialize, &[h.ctx().string("hello\tworld")]);
    assert_eq!(result.unwrap(), h.ctx().string("\"hello\\tworld\""));
}

#[test]
fn test_json_serialize_pretty() {
    let (_vm, mut symbols, meta) = setup();
    let json_serialize_pretty = get_primitive(&meta, &mut symbols, "json-serialize-pretty");
    let h = elle::primitives::ctx::TestHeap::new();

    let list = h
        .ctx()
        .list(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let result = call_primitive(&json_serialize_pretty, &[list]);
    let serialized = result.unwrap();
    match serialized {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert!(s.contains('\n'), "Pretty JSON should contain newlines");
            assert!(s.contains("  "), "Pretty JSON should contain indentation");
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_json_serialize_roundtrip() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let json_serialize = get_primitive(&meta, &mut symbols, "json-serialize");
    let h = elle::primitives::ctx::TestHeap::new();

    let original = h.ctx().list(vec![
        Value::int(1),
        h.ctx().string("test"),
        Value::bool(true),
        Value::NIL,
    ]);

    let serialized = call_primitive(&json_serialize, std::slice::from_ref(&original)).unwrap();
    let json_str = if let Some(s) = serialized.with_string(|s| s.to_string()) {
        s
    } else {
        panic!("Expected string");
    };

    let deserialized = call_primitive(&json_parse, &[h.ctx().string(json_str)]).unwrap();
    assert_eq!(original, deserialized);
}

#[test]
fn test_json_serialize_arrays() {
    let (_vm, mut symbols, meta) = setup();
    let json_serialize = get_primitive(&meta, &mut symbols, "json-serialize");
    let h = elle::primitives::ctx::TestHeap::new();

    let vec = h
        .ctx()
        .array_mut(vec![Value::int(1), Value::int(2), Value::int(3)]);
    let result = call_primitive(&json_serialize, &[vec]);
    assert_eq!(result.unwrap(), h.ctx().string("[1,2,3]"));
}

#[test]
fn test_json_serialize_errors() {
    let (_vm, mut symbols, meta) = setup();
    let json_serialize = get_primitive(&meta, &mut symbols, "json-serialize");
    let h = elle::primitives::ctx::TestHeap::new();

    let closure = h.ctx().closure(Closure {
        template: std::rc::Rc::new(ClosureTemplate::new(
            std::rc::Rc::new(vec![]),
            elle::value::Arity::Exact(0),
            std::rc::Rc::new(vec![]),
        ))
        .into(),
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });
    let result = call_primitive(&json_serialize, &[closure]);
    assert!(result.is_err());

    let fn_val = Value::native_fn(&elle::primitives::def::NOOP_PRIM);
    let result = call_primitive(&json_serialize, &[fn_val]);
    assert!(result.is_err());
}
