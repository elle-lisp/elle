//! JSON parsing and serialization primitives
//!
//! Provides hand-written recursive descent JSON parser and serializer.
//! No external JSON libraries - all implemented directly.

mod parser;
mod serializer;

pub use parser::JsonParser;
pub use serializer::{escape_json_string, serialize_value, serialize_value_pretty};

use crate::value::{Condition, Value};

/// Parse a JSON string into Elle values
pub fn prim_json_parse(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(
            "json-parse: expected 1 argument".to_string(),
        ));
    }

    let json_str = if let Some(s) = args[0].as_string() {
        s
    } else {
        return Err(Condition::type_error(
            "json-parse: expected string argument".to_string(),
        ));
    };

    let mut parser = JsonParser::new(json_str);
    parser.parse().map_err(Condition::error)
}

/// Serialize an Elle value to compact JSON
pub fn prim_json_serialize(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(
            "json-serialize: expected 1 argument".to_string(),
        ));
    }

    let json_str = serialize_value(&args[0]).map_err(Condition::error)?;
    Ok(Value::string(json_str))
}

/// Serialize an Elle value to pretty-printed JSON with 2-space indentation
pub fn prim_json_serialize_pretty(args: &[Value]) -> Result<Value, Condition> {
    if args.len() != 1 {
        return Err(Condition::arity_error(
            "json-serialize-pretty: expected 1 argument".to_string(),
        ));
    }

    let json_str = serialize_value_pretty(&args[0], 0).map_err(Condition::error)?;
    Ok(Value::string(json_str))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::value::list;
    use std::collections::BTreeMap;
    use std::rc::Rc;

    #[test]
    fn test_parse_null() {
        let mut parser = JsonParser::new("null");
        assert_eq!(parser.parse().unwrap(), Value::NIL);
    }

    #[test]
    fn test_parse_booleans() {
        let mut parser = JsonParser::new("true");
        assert_eq!(parser.parse().unwrap(), Value::bool(true));

        let mut parser = JsonParser::new("false");
        assert_eq!(parser.parse().unwrap(), Value::bool(false));
    }

    #[test]
    fn test_parse_integers() {
        let mut parser = JsonParser::new("0");
        assert_eq!(parser.parse().unwrap(), Value::int(0));

        let mut parser = JsonParser::new("42");
        assert_eq!(parser.parse().unwrap(), Value::int(42));

        let mut parser = JsonParser::new("-17");
        assert_eq!(parser.parse().unwrap(), Value::int(-17));

        let mut parser = JsonParser::new("140737488355327");
        assert_eq!(parser.parse().unwrap(), Value::int(140737488355327));
    }

    #[test]
    #[allow(clippy::approx_constant)]
    fn test_parse_floats() {
        let mut parser = JsonParser::new("3.14");
        if let Some(f) = parser.parse().unwrap().as_float() {
            assert!((f - 3.14).abs() < 1e-10);
        } else {
            panic!("Expected float");
        }

        let mut parser = JsonParser::new("-0.5");
        if let Some(f) = parser.parse().unwrap().as_float() {
            assert!((f - (-0.5)).abs() < 1e-10);
        } else {
            panic!("Expected float");
        }

        let mut parser = JsonParser::new("1e10");
        if let Some(f) = parser.parse().unwrap().as_float() {
            assert!((f - 1e10).abs() < 1e5);
        } else {
            panic!("Expected float");
        }

        let mut parser = JsonParser::new("2.5e-3");
        if let Some(f) = parser.parse().unwrap().as_float() {
            assert!((f - 0.0025).abs() < 1e-10);
        } else {
            panic!("Expected float");
        }

        let mut parser = JsonParser::new("1.0");
        if let Some(f) = parser.parse().unwrap().as_float() {
            assert!((f - 1.0).abs() < 1e-10);
        } else {
            panic!("Expected float");
        }
    }

    #[test]
    fn test_parse_strings() {
        let mut parser = JsonParser::new("\"hello\"");
        assert_eq!(parser.parse().unwrap(), Value::string("hello"));

        let mut parser = JsonParser::new("\"\"");
        assert_eq!(parser.parse().unwrap(), Value::string(""));

        let mut parser = JsonParser::new("\"hello\\nworld\"");
        assert_eq!(parser.parse().unwrap(), Value::string("hello\nworld"));

        let mut parser = JsonParser::new("\"quote\\\"test\"");
        assert_eq!(parser.parse().unwrap(), Value::string("quote\"test"));

        let mut parser = JsonParser::new("\"backslash\\\\test\"");
        assert_eq!(parser.parse().unwrap(), Value::string("backslash\\test"));

        let mut parser = JsonParser::new("\"tab\\there\"");
        assert_eq!(parser.parse().unwrap(), Value::string("tab\there"));

        let mut parser = JsonParser::new("\"\\u0041\"");
        assert_eq!(parser.parse().unwrap(), Value::string("A"));
    }

    #[test]
    fn test_parse_arrays() {
        let mut parser = JsonParser::new("[]");
        assert_eq!(parser.parse().unwrap(), Value::EMPTY_LIST);

        let mut parser = JsonParser::new("[1,2,3]");
        let result = parser.parse().unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
        assert_eq!(vec[0], Value::int(1));
        assert_eq!(vec[1], Value::int(2));
        assert_eq!(vec[2], Value::int(3));

        let mut parser = JsonParser::new("[1,\"two\",true,null]");
        let result = parser.parse().unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 4);
        assert_eq!(vec[0], Value::int(1));
        assert_eq!(vec[1], Value::string("two"));
        assert_eq!(vec[2], Value::bool(true));
        assert_eq!(vec[3], Value::NIL);
    }

    #[test]
    fn test_parse_objects() {
        let mut parser = JsonParser::new("{}");
        if let Some(t) = parser.parse().unwrap().as_table() {
            assert_eq!(t.borrow().len(), 0);
        } else {
            panic!("Expected table");
        }

        let mut parser = JsonParser::new("{\"name\":\"Alice\",\"age\":30}");
        if let Some(t) = parser.parse().unwrap().as_table() {
            let table = t.borrow();
            assert_eq!(table.len(), 2);
            assert_eq!(
                table.get(&crate::value::TableKey::String("name".to_string())),
                Some(&Value::string("Alice"))
            );
            assert_eq!(
                table.get(&crate::value::TableKey::String("age".to_string())),
                Some(&Value::int(30))
            );
        } else {
            panic!("Expected table");
        }
    }
    #[test]
    fn test_parse_whitespace() {
        let mut parser = JsonParser::new("  \n\t  42  \n\t  ");
        assert_eq!(parser.parse().unwrap(), Value::int(42));

        let mut parser = JsonParser::new("[ 1 , 2 , 3 ]");
        let result = parser.parse().unwrap();
        let vec = result.list_to_vec().unwrap();
        assert_eq!(vec.len(), 3);
    }

    #[test]
    fn test_parse_errors() {
        let mut parser = JsonParser::new("");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("42 extra");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("\"unterminated");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("[1,2");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("{\"key\":42");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("invalid");
        assert!(parser.parse().is_err());
    }

    #[test]
    fn test_serialize_compact() {
        assert_eq!(serialize_value(&Value::NIL).unwrap(), "null");
        assert_eq!(serialize_value(&Value::bool(true)).unwrap(), "true");
        assert_eq!(serialize_value(&Value::bool(false)).unwrap(), "false");
        assert_eq!(serialize_value(&Value::int(42)).unwrap(), "42");
        assert_eq!(serialize_value(&Value::int(-17)).unwrap(), "-17");

        #[allow(clippy::approx_constant)]
        {
            match serialize_value(&Value::float(3.14)) {
                Ok(s) => assert!(s.contains("3.14")),
                Err(e) => panic!("Error: {}", e),
            }
        }

        assert_eq!(
            serialize_value(&Value::string("hello")).unwrap(),
            "\"hello\""
        );

        let list = list(vec![Value::int(1), Value::int(2), Value::int(3)]);
        assert_eq!(serialize_value(&list).unwrap(), "[1,2,3]");

        let mut map = BTreeMap::new();
        map.insert(
            crate::value::TableKey::String("name".to_string()),
            Value::string("Alice"),
        );
        map.insert(
            crate::value::TableKey::String("age".to_string()),
            Value::int(30),
        );
        let table = Value::table_from(map);
        let serialized = serialize_value(&table).unwrap();
        assert!(serialized.contains("\"name\":\"Alice\""));
        assert!(serialized.contains("\"age\":30"));
    }

    #[test]
    fn test_serialize_string_escaping() {
        assert_eq!(
            serialize_value(&Value::string("hello\"world")).unwrap(),
            "\"hello\\\"world\""
        );

        assert_eq!(
            serialize_value(&Value::string("hello\\world")).unwrap(),
            "\"hello\\\\world\""
        );

        assert_eq!(
            serialize_value(&Value::string("hello\nworld")).unwrap(),
            "\"hello\\nworld\""
        );

        assert_eq!(
            serialize_value(&Value::string("hello\tworld")).unwrap(),
            "\"hello\\tworld\""
        );
    }

    #[test]
    fn test_serialize_roundtrip() {
        let original = list(vec![
            Value::int(1),
            Value::string("test"),
            Value::bool(true),
            Value::NIL,
        ]);

        let serialized = serialize_value(&original).unwrap();
        let mut parser = JsonParser::new(&serialized);
        let deserialized = parser.parse().unwrap();

        assert_eq!(original, deserialized);
    }

    #[test]
    fn test_serialize_pretty() {
        let list = list(vec![Value::int(1), Value::int(2), Value::int(3)]);
        let pretty = serialize_value_pretty(&list, 0).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));

        let mut map = BTreeMap::new();
        map.insert(
            crate::value::TableKey::String("key".to_string()),
            Value::int(42),
        );
        let table = Value::table_from(map);
        let pretty = serialize_value_pretty(&table, 0).unwrap();
        assert!(pretty.contains('\n'));
        assert!(pretty.contains("  "));
    }

    #[test]
    fn test_serialize_errors() {
        let closure = Value::closure(crate::value::Closure {
            bytecode: Rc::new(vec![]),
            arity: crate::value::Arity::Exact(0),
            env: Rc::new(vec![]),
            num_locals: 0,
            num_captures: 0,
            constants: Rc::new(vec![]),
            effect: crate::effects::Effect::pure(),
            cell_params_mask: 0,
            symbol_names: Rc::new(std::collections::HashMap::new()),
            location_map: Rc::new(crate::error::LocationMap::new()),
            jit_code: None,
            lir_function: None,
        });
        assert!(serialize_value(&closure).is_err());

        let native_fn: crate::value::NativeFn = |_| Ok(Value::NIL);
        let fn_val = Value::native_fn(native_fn);
        assert!(serialize_value(&fn_val).is_err());
    }

    #[test]
    fn test_float_formatting() {
        match serialize_value(&Value::float(1.0)) {
            Ok(s) => assert!(
                s.contains("."),
                "Float 1.0 should contain decimal point, got: {}",
                s
            ),
            Err(e) => panic!("Error: {}", e),
        }

        match serialize_value(&Value::float(42.0)) {
            Ok(s) => assert!(
                s.contains("."),
                "Float 42.0 should contain decimal point, got: {}",
                s
            ),
            Err(e) => panic!("Error: {}", e),
        }
    }

    #[test]
    fn test_parse_leading_zeros() {
        // Leading zeros are not allowed in JSON
        let mut parser = JsonParser::new("01");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("00");
        assert!(parser.parse().is_err());

        // But "0" alone is valid
        let mut parser = JsonParser::new("0");
        assert_eq!(parser.parse().unwrap(), Value::int(0));

        // And "0.1" is valid
        let mut parser = JsonParser::new("0.1");
        if let Some(f) = parser.parse().unwrap().as_float() {
            assert!((f - 0.1).abs() < 1e-10);
        } else {
            panic!("Expected float");
        }
    }

    #[test]
    fn test_parse_trailing_comma() {
        // Trailing commas are not allowed in JSON
        let mut parser = JsonParser::new("[1,2,]");
        assert!(parser.parse().is_err());

        let mut parser = JsonParser::new("{\"a\":1,}");
        assert!(parser.parse().is_err());
    }

    #[test]
    fn test_serialize_nan_infinity() {
        // NaN should error
        assert!(serialize_value(&Value::float(f64::NAN)).is_err());

        // Positive infinity should error
        assert!(serialize_value(&Value::float(f64::INFINITY)).is_err());

        // Negative infinity should error
        assert!(serialize_value(&Value::float(f64::NEG_INFINITY)).is_err());
    }

    #[test]
    fn test_serialize_non_string_table_key() {
        let mut map = BTreeMap::new();
        map.insert(crate::value::TableKey::Int(42), Value::string("value"));
        let table = Value::table_from(map);

        // Should error because key is not a string
        assert!(serialize_value(&table).is_err());
    }

    #[test]
    fn test_json_parse_wrong_type() {
        // json-parse requires a string argument
        let result = prim_json_parse(&[Value::int(42)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_json_serialize_wrong_arity() {
        // json-serialize requires exactly 1 argument
        let result = prim_json_serialize(&[]);
        assert!(result.is_err());

        let result = prim_json_serialize(&[Value::int(1), Value::int(2)]);
        assert!(result.is_err());
    }

    #[test]
    fn test_parse_surrogate_pair() {
        // Emoji 😀 is U+1F600, encoded as surrogate pair \uD83D\uDE00
        let mut parser = JsonParser::new("\"\\uD83D\\uDE00\"");
        if let Some(s) = parser.parse().unwrap().as_string() {
            assert_eq!(s, "😀");
        } else {
            panic!("Expected string");
        }
    }

    #[test]
    fn test_parse_lone_surrogate() {
        // High surrogate without low surrogate should error
        let mut parser = JsonParser::new("\"\\uD800\"");
        assert!(parser.parse().is_err());

        // Low surrogate without high surrogate should error
        let mut parser = JsonParser::new("\"\\uDC00\"");
        assert!(parser.parse().is_err());
    }
}
