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
    assert_eq!(result.unwrap(), h.ctx().array(vec![]));

    let result = call_primitive(&json_parse, &[h.ctx().string("[1,2,3]")]);
    let array = result.unwrap();
    assert!(
        array.as_array_mut().is_none(),
        "a parsed JSON array is immutable"
    );
    let elements = array.as_array().expect("JSON array parses to an array");
    assert_eq!(elements.len(), 3);
    assert_eq!(elements[0], Value::int(1));
    assert_eq!(elements[1], Value::int(2));
    assert_eq!(elements[2], Value::int(3));

    let result = call_primitive(&json_parse, &[h.ctx().string("[1,\"two\",true,null]")]);
    let array = result.unwrap();
    let elements = array.as_array().expect("JSON array parses to an array");
    assert_eq!(elements.len(), 4);
    assert_eq!(elements[0], Value::int(1));
    assert_eq!(elements[1], h.ctx().string("two"));
    assert_eq!(elements[2], Value::bool(true));
    assert_eq!(elements[3], Value::NIL);
}

/// Read one string-keyed field out of an immutable struct.
fn json_field(owner: Value, name: &str) -> Value {
    owner
        .as_struct()
        .unwrap_or_else(|| panic!("expected an immutable struct, looking for {:?}", name))
        .iter()
        .find(|(k, _)| k.as_str() == Some(name))
        .map(|(_, v)| *v)
        .unwrap_or_else(|| panic!("no field {:?}", name))
}

/// Every collection inside a parsed document is immutable too, not just the
/// value the parser returns at the top.
#[test]
fn test_json_parse_is_immutable_at_every_depth() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("{\"a\": [1, {\"b\": [2]}]}")]);
    let root = result.unwrap();
    assert!(root.as_struct_mut().is_none(), "root object is immutable");

    let outer = json_field(root, "a");
    assert!(outer.as_array_mut().is_none(), "nested array is immutable");
    let outer = outer.as_array().expect("nested array parses to an array");

    let inner = outer[1];
    assert!(
        inner.as_struct_mut().is_none(),
        "nested object is immutable"
    );

    let deepest = json_field(inner, "b");
    assert!(
        deepest.as_array_mut().is_none(),
        "deepest array is immutable"
    );
    assert_eq!(
        deepest.as_array().expect("deepest value is an array"),
        &[Value::int(2)]
    );
}

#[test]
fn test_json_parse_objects() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("{}")]);
    let empty = result.unwrap();
    assert!(
        empty.as_struct_mut().is_none(),
        "a parsed JSON object is immutable"
    );
    assert_eq!(
        empty
            .as_struct()
            .expect("JSON object parses to a struct")
            .len(),
        0
    );

    let result = call_primitive(
        &json_parse,
        &[h.ctx().string("{\"name\":\"Alice\",\"age\":30}")],
    );
    let parsed = result.unwrap();
    assert!(
        parsed.as_struct_mut().is_none(),
        "a parsed JSON object is immutable"
    );
    let fields = parsed.as_struct().expect("JSON object parses to a struct");
    assert_eq!(fields.len(), 2);
    assert_eq!(json_field(parsed, "name"), h.ctx().string("Alice"));
    assert_eq!(json_field(parsed, "age"), Value::int(30));
}

#[test]
fn test_json_parse_whitespace() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let h = elle::primitives::ctx::TestHeap::new();

    let result = call_primitive(&json_parse, &[h.ctx().string("  \n\t  42  \n\t  ")]);
    assert_eq!(result.unwrap(), Value::int(42));

    let result = call_primitive(&json_parse, &[h.ctx().string("[ 1 , 2 , 3 ]")]);
    assert_eq!(
        result.unwrap(),
        h.ctx()
            .array(vec![Value::int(1), Value::int(2), Value::int(3)])
    );
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

    // An immutable array is the fixed point of the roundtrip: it serializes to
    // a JSON array, and a JSON array parses back to an immutable array. A list
    // serializes the same way but does not survive the return trip unchanged.
    let original = h.ctx().array(vec![
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

/// A JSON document with nested objects and arrays survives a serialize/parse
/// roundtrip with its shape intact.
#[test]
fn test_json_nested_roundtrip() {
    let (_vm, mut symbols, meta) = setup();
    let json_parse = get_primitive(&meta, &mut symbols, "json-parse");
    let json_serialize = get_primitive(&meta, &mut symbols, "json-serialize");
    let h = elle::primitives::ctx::TestHeap::new();

    let source = "{\"a\":[1,[2,3]],\"b\":{\"c\":null}}";
    let parsed = call_primitive(&json_parse, &[h.ctx().string(source)]).unwrap();
    let serialized = call_primitive(&json_serialize, std::slice::from_ref(&parsed)).unwrap();
    assert_eq!(serialized, h.ctx().string(source));

    let reparsed = call_primitive(&json_parse, &[serialized]).unwrap();
    assert_eq!(parsed, reparsed);
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
        template: {
            let region = h.heap().new_runtime_region();
            elle::value::TemplateRef::region(elle::value::closure::materialize(
                h.heap(),
                &std::rc::Rc::new(elle::value::TemplateProto::new(
                    vec![],
                    elle::value::Arity::Exact(0),
                    Vec::new(),
                )),
                region,
            ))
        },
        env: elle::value::region_slice::RegionSlice::empty(),
        squelch_mask: SignalBits::EMPTY,
    });
    let result = call_primitive(&json_serialize, &[closure]);
    assert!(result.is_err());

    let fn_val = Value::native_fn(&elle::primitives::def::NOOP_PRIM);
    let result = call_primitive(&json_serialize, &[fn_val]);
    assert!(result.is_err());
}
