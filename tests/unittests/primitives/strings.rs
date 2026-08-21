use super::*;

// Standard library tests

#[test]
fn test_string_module_functions() {
    let (_vm, mut symbols, meta) = setup();
    let h = elle::primitives::ctx::TestHeap::new();

    // Test length on strings
    let length_fn = get_primitive(&meta, &mut symbols, "length");
    let str_val = h.ctx().string("hello");
    assert_eq!(
        call_primitive(&length_fn, &[str_val]).unwrap(),
        Value::int(5)
    );

    // Test string-upcase
    let upcase_fn = get_primitive(&meta, &mut symbols, "string-upcase");
    let str_val = h.ctx().string("hello");
    match call_primitive(&upcase_fn, &[str_val]).unwrap() {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "HELLO")
        }
        _ => panic!("Expected string"),
    }

    // Test string-downcase
    let downcase_fn = get_primitive(&meta, &mut symbols, "string-downcase");
    let str_val = h.ctx().string("HELLO");
    match call_primitive(&downcase_fn, &[str_val]).unwrap() {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_split() {
    let (_vm, mut symbols, meta) = setup();
    let split_fn = get_primitive(&meta, &mut symbols, "string-split");
    let h = elle::primitives::ctx::TestHeap::new();

    // Basic split
    let result =
        call_primitive(&split_fn, &[h.ctx().string("a,b,c"), h.ctx().string(",")]).unwrap();
    assert!(result.is_array());
    let vec = result.as_array().unwrap();
    assert_eq!(vec.len(), 3);
    assert_eq!(vec[0], h.ctx().string("a"));
    assert_eq!(vec[1], h.ctx().string("b"));
    assert_eq!(vec[2], h.ctx().string("c"));

    // Split with multi-char delimiter
    let result =
        call_primitive(&split_fn, &[h.ctx().string("hello"), h.ctx().string("ll")]).unwrap();
    let vec = result.as_array().unwrap();
    assert_eq!(vec.len(), 2);
    assert_eq!(vec[0], h.ctx().string("he"));
    assert_eq!(vec[1], h.ctx().string("o"));

    // No match returns original in tuple
    let result =
        call_primitive(&split_fn, &[h.ctx().string("hello"), h.ctx().string("xyz")]).unwrap();
    let vec = result.as_array().unwrap();
    assert_eq!(vec.len(), 1);
    assert_eq!(vec[0], h.ctx().string("hello"));
}

#[test]
fn test_string_replace() {
    let (_vm, mut symbols, meta) = setup();
    let replace_fn = get_primitive(&meta, &mut symbols, "string-replace");
    let h = elle::primitives::ctx::TestHeap::new();

    // Basic replace
    let result = call_primitive(
        &replace_fn,
        &[
            h.ctx().string("hello world"),
            h.ctx().string("world"),
            h.ctx().string("elle"),
        ],
    )
    .unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "hello elle")
        }
        _ => panic!("Expected string"),
    }

    // Replace all occurrences
    let result = call_primitive(
        &replace_fn,
        &[
            h.ctx().string("aaa"),
            h.ctx().string("a"),
            h.ctx().string("bb"),
        ],
    )
    .unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "bbbbbb")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_trim() {
    let (_vm, mut symbols, meta) = setup();
    let trim_fn = get_primitive(&meta, &mut symbols, "string-trim");
    let h = elle::primitives::ctx::TestHeap::new();

    // Trim whitespace
    let result = call_primitive(&trim_fn, &[h.ctx().string("  hello  ")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }

    // No whitespace
    let result = call_primitive(&trim_fn, &[h.ctx().string("hello")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_string_contains() {
    let (_vm, mut symbols, meta) = setup();
    let contains_fn = get_primitive(&meta, &mut symbols, "string-contains?");
    let h = elle::primitives::ctx::TestHeap::new();

    // Contains substring
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[h.ctx().string("hello world"), h.ctx().string("world"),]
        )
        .unwrap(),
        Value::bool(true)
    );

    // Does not contain
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[h.ctx().string("hello"), h.ctx().string("xyz"),]
        )
        .unwrap(),
        Value::bool(false)
    );

    // Empty string is contained in everything
    assert_eq!(
        call_primitive(
            &contains_fn,
            &[h.ctx().string("hello"), h.ctx().string(""),]
        )
        .unwrap(),
        Value::bool(true)
    );
}

#[test]
fn test_string_starts_with() {
    let (_vm, mut symbols, meta) = setup();
    let starts_fn = get_primitive(&meta, &mut symbols, "string-starts-with?");
    let h = elle::primitives::ctx::TestHeap::new();

    // Starts with
    assert_eq!(
        call_primitive(
            &starts_fn,
            &[h.ctx().string("hello"), h.ctx().string("hel"),]
        )
        .unwrap(),
        Value::bool(true)
    );

    // Does not start with
    assert_eq!(
        call_primitive(
            &starts_fn,
            &[h.ctx().string("hello"), h.ctx().string("world"),]
        )
        .unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_string_ends_with() {
    let (_vm, mut symbols, meta) = setup();
    let ends_fn = get_primitive(&meta, &mut symbols, "string-ends-with?");
    let h = elle::primitives::ctx::TestHeap::new();

    // Ends with
    assert_eq!(
        call_primitive(&ends_fn, &[h.ctx().string("hello"), h.ctx().string("llo"),]).unwrap(),
        Value::bool(true)
    );

    // Does not end with
    assert_eq!(
        call_primitive(
            &ends_fn,
            &[h.ctx().string("hello"), h.ctx().string("world"),]
        )
        .unwrap(),
        Value::bool(false)
    );
}

#[test]
fn test_string_join() {
    let (_vm, mut symbols, meta) = setup();
    let join_fn = get_primitive(&meta, &mut symbols, "string-join");
    let h = elle::primitives::ctx::TestHeap::new();

    // Join list of strings
    let list_val = h.ctx().list(vec![
        h.ctx().string("a"),
        h.ctx().string("b"),
        h.ctx().string("c"),
    ]);
    let result = call_primitive(&join_fn, &[list_val, h.ctx().string(",")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "a,b,c")
        }
        _ => panic!("Expected string"),
    }

    // Single element
    let list_val = h.ctx().list(vec![h.ctx().string("hello")]);
    let result = call_primitive(&join_fn, &[list_val, h.ctx().string(" ")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "hello")
        }
        _ => panic!("Expected string"),
    }

    // Empty list
    let list_val = h.ctx().list(vec![]);
    let result = call_primitive(&join_fn, &[list_val, h.ctx().string(",")]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "")
        }
        _ => panic!("Expected string"),
    }
}

#[test]
fn test_number_to_string() {
    let (_vm, mut symbols, meta) = setup();
    let num_to_str = get_primitive(&meta, &mut symbols, "number->string");
    let h = elle::primitives::ctx::TestHeap::new();

    // Integer to string
    let result = call_primitive(&num_to_str, &[Value::int(42)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "42")
        }
        _ => panic!("Expected string"),
    }

    // Float to string
    let result = call_primitive(&num_to_str, &[Value::float(std::f64::consts::PI)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            // Just check that it starts with "3.14" since float representation may vary
            assert!(s.starts_with("3.14"));
        }
        _ => panic!("Expected string"),
    }

    // Negative numbers
    let result = call_primitive(&num_to_str, &[Value::int(-42)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "-42")
        }
        _ => panic!("Expected string"),
    }

    // Zero
    let result = call_primitive(&num_to_str, &[Value::int(0)]).unwrap();
    match result {
        v if v.is_string() => {
            let s = v.with_string(|s| s.to_string()).unwrap();
            assert_eq!(s, "0")
        }
        _ => panic!("Expected string"),
    }

    // Radix: hex (base 16)
    let result = call_primitive(&num_to_str, &[Value::int(255), Value::int(16)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "ff");

    // Radix: binary (base 2)
    let result = call_primitive(&num_to_str, &[Value::int(255), Value::int(2)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "11111111");

    // Radix: octal (base 8)
    let result = call_primitive(&num_to_str, &[Value::int(255), Value::int(8)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "377");

    // Radix: base 36
    let result = call_primitive(&num_to_str, &[Value::int(35), Value::int(36)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "z");

    // Radix: negative value
    let result = call_primitive(&num_to_str, &[Value::int(-255), Value::int(16)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "-ff");

    // Radix: zero
    let result = call_primitive(&num_to_str, &[Value::int(0), Value::int(16)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "0");

    // Radix: explicit decimal
    let result = call_primitive(&num_to_str, &[Value::int(10), Value::int(10)]).unwrap();
    assert_eq!(result.with_string(|s| s.to_string()).unwrap(), "10");

    // Error: float with radix
    assert!(call_primitive(&num_to_str, &[Value::float(3.5), Value::int(16)]).is_err());

    // Error: radix out of range (too low)
    assert!(call_primitive(&num_to_str, &[Value::int(42), Value::int(1)]).is_err());

    // Error: radix out of range (too high)
    assert!(call_primitive(&num_to_str, &[Value::int(42), Value::int(37)]).is_err());

    // Error: non-number first arg
    assert!(call_primitive(&num_to_str, &[h.ctx().string("hello")]).is_err());
}

#[test]
fn test_string_split_errors() {
    let (_vm, mut symbols, meta) = setup();
    let split_fn = get_primitive(&meta, &mut symbols, "string-split");
    let h = elle::primitives::ctx::TestHeap::new();

    // Wrong type - first arg not string
    assert!(call_primitive(&split_fn, &[Value::int(42), h.ctx().string(","),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&split_fn, &[h.ctx().string("hello"), Value::int(42),]).is_err());

    // Empty delimiter
    assert!(call_primitive(&split_fn, &[h.ctx().string("hello"), h.ctx().string(""),]).is_err());
}

#[test]
fn test_string_replace_errors() {
    let (_vm, mut symbols, meta) = setup();
    let replace_fn = get_primitive(&meta, &mut symbols, "string-replace");
    let h = elle::primitives::ctx::TestHeap::new();

    // Wrong type - first arg not string
    assert!(call_primitive(
        &replace_fn,
        &[Value::int(42), h.ctx().string("l"), h.ctx().string("x"),]
    )
    .is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(
        &replace_fn,
        &[h.ctx().string("hello"), Value::int(42), h.ctx().string("x"),]
    )
    .is_err());

    // Wrong type - third arg not string
    assert!(call_primitive(
        &replace_fn,
        &[h.ctx().string("hello"), h.ctx().string("l"), Value::int(42),]
    )
    .is_err());

    // Empty search string
    assert!(call_primitive(
        &replace_fn,
        &[
            h.ctx().string("hello"),
            h.ctx().string(""),
            h.ctx().string("x"),
        ]
    )
    .is_err());
}

#[test]
fn test_string_trim_errors() {
    let (_vm, mut symbols, meta) = setup();
    let trim_fn = get_primitive(&meta, &mut symbols, "string-trim");

    // Wrong type - not string
    assert!(call_primitive(&trim_fn, &[Value::int(42)]).is_err());
}

#[test]
fn test_string_contains_errors() {
    let (_vm, mut symbols, meta) = setup();
    let contains_fn = get_primitive(&meta, &mut symbols, "string-contains?");
    let h = elle::primitives::ctx::TestHeap::new();

    // Wrong type - first arg not string
    assert!(call_primitive(&contains_fn, &[Value::int(42), h.ctx().string("l"),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&contains_fn, &[h.ctx().string("hello"), Value::int(42),]).is_err());
}

#[test]
fn test_string_starts_with_errors() {
    let (_vm, mut symbols, meta) = setup();
    let starts_fn = get_primitive(&meta, &mut symbols, "string-starts-with?");
    let h = elle::primitives::ctx::TestHeap::new();

    // Wrong type - first arg not string
    assert!(call_primitive(&starts_fn, &[Value::int(42), h.ctx().string("h"),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&starts_fn, &[h.ctx().string("hello"), Value::int(42),]).is_err());
}

#[test]
fn test_string_ends_with_errors() {
    let (_vm, mut symbols, meta) = setup();
    let ends_fn = get_primitive(&meta, &mut symbols, "string-ends-with?");
    let h = elle::primitives::ctx::TestHeap::new();

    // Wrong type - first arg not string
    assert!(call_primitive(&ends_fn, &[Value::int(42), h.ctx().string("o"),]).is_err());

    // Wrong type - second arg not string
    assert!(call_primitive(&ends_fn, &[h.ctx().string("hello"), Value::int(42),]).is_err());
}

#[test]
fn test_string_join_errors() {
    let (_vm, mut symbols, meta) = setup();
    let join_fn = get_primitive(&meta, &mut symbols, "string-join");
    let h = elle::primitives::ctx::TestHeap::new();

    // Wrong type - second arg not string
    assert!(call_primitive(&join_fn, &[h.ctx().list(vec![]), Value::int(42),]).is_err());

    // Non-string list elements
    let list_val = h.ctx().list(vec![
        h.ctx().string("a"),
        Value::int(42),
        h.ctx().string("c"),
    ]);
    assert!(call_primitive(&join_fn, &[list_val, h.ctx().string(",")]).is_err());
}
