// Integration tests for new/refactored primitive modules
use crate::common::eval_source;
use elle::Value;

// === Read primitives ===

#[test]
fn test_read_integer() {
    assert_eq!(eval_source("(read \"42\")").unwrap(), Value::int(42));
}

#[test]
fn test_read_string() {
    assert_eq!(
        eval_source("(read \"\\\"hello\\\"\")").unwrap(),
        Value::string("hello")
    );
}

#[test]
fn test_read_boolean() {
    assert_eq!(eval_source("(read \"true\")").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(read \"false\")").unwrap(), Value::FALSE);
}

#[test]
fn test_read_list() {
    // read should parse a list form
    let result = eval_source("(read \"(+ 1 2)\")").unwrap();
    assert!(result.as_cons().is_some(), "Expected a cons cell (list)");
}

#[test]
fn test_read_type_error() {
    let result = eval_source("(read 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_read_all_multiple_forms() {
    let result = eval_source("(read-all \"1 2 3\")").unwrap();
    // Should return a list of three values
    let first = result.as_cons().expect("should be a list");
    assert_eq!(first.first, Value::int(1));
}

#[test]
fn test_read_all_empty() {
    let result = eval_source("(read-all \"\")").unwrap();
    assert_eq!(result, Value::EMPTY_LIST);
}

#[test]
fn test_read_all_type_error() {
    let result = eval_source("(read-all 42)");
    assert!(result.is_err());
}

// === Conversion primitives ===

#[test]
fn test_integer_from_int() {
    assert_eq!(eval_source("(integer 42)").unwrap(), Value::int(42));
}

#[test]
fn test_integer_from_float() {
    assert_eq!(eval_source("(integer 3.7)").unwrap(), Value::int(3));
}

#[test]
fn test_integer_from_string() {
    assert_eq!(eval_source("(integer \"42\")").unwrap(), Value::int(42));
}

#[test]
fn test_integer_from_bad_string() {
    let result = eval_source("(integer \"abc\")");
    assert!(result.is_err());
}

#[test]
fn test_integer_type_error() {
    let result = eval_source("(integer true)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_float_from_int() {
    assert_eq!(eval_source("(float 42)").unwrap(), Value::float(42.0));
}

#[test]
fn test_float_from_float() {
    assert_eq!(eval_source("(float 2.5)").unwrap(), Value::float(2.5));
}

#[test]
fn test_float_from_string() {
    assert_eq!(eval_source("(float \"2.5\")").unwrap(), Value::float(2.5));
}

#[test]
fn test_float_from_bad_string() {
    let result = eval_source("(float \"abc\")");
    assert!(result.is_err());
}

#[test]
fn test_string_from_int() {
    assert_eq!(eval_source("(string 42)").unwrap(), Value::string("42"));
}

#[test]
fn test_string_from_float() {
    // Float formatting may vary, just check it's a string
    let result = eval_source("(string 3.14)").unwrap();
    assert!(result.is_string());
}

#[test]
fn test_string_from_bool() {
    assert_eq!(eval_source("(string true)").unwrap(), Value::string("true"));
    assert_eq!(
        eval_source("(string false)").unwrap(),
        Value::string("false")
    );
}

#[test]
fn test_string_from_nil() {
    assert_eq!(eval_source("(string nil)").unwrap(), Value::string("nil"));
}

#[test]
fn test_string_from_list() {
    let result = eval_source("(string (list 1 2 3))").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be a string");
    assert_eq!(s, "(1 2 3)");
}

#[test]
fn test_string_from_array() {
    let result = eval_source("(string @[1 2 3])").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be a string");
    assert_eq!(s, "[1, 2, 3]");
}

#[test]
fn test_number_to_string_int() {
    assert_eq!(
        eval_source("(number->string 42)").unwrap(),
        Value::string("42")
    );
}

#[test]
fn test_number_to_string_float() {
    let result = eval_source("(number->string 3.14)").unwrap();
    assert!(result.is_string());
}

#[test]
fn test_number_to_string_type_error() {
    let result = eval_source("(number->string \"hello\")");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_string_to_integer() {
    assert_eq!(
        eval_source("(string->integer \"42\")").unwrap(),
        Value::int(42)
    );
}

#[test]
fn test_string_to_integer_negative() {
    assert_eq!(
        eval_source("(string->integer \"-7\")").unwrap(),
        Value::int(-7)
    );
}

#[test]
fn test_string_to_float() {
    assert_eq!(
        eval_source("(string->float \"2.5\")").unwrap(),
        Value::float(2.5)
    );
}

#[test]
fn test_any_to_string() {
    assert_eq!(
        eval_source("(any->string 42)").unwrap(),
        Value::string("42")
    );
    assert_eq!(
        eval_source("(any->string true)").unwrap(),
        Value::string("true")
    );
}

#[test]
fn test_keyword_to_string() {
    assert_eq!(
        eval_source("(keyword->string :foo)").unwrap(),
        Value::string("foo")
    );
}

#[test]
fn test_keyword_to_string_type_error() {
    let result = eval_source("(keyword->string 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_symbol_to_string() {
    assert_eq!(
        eval_source("(symbol->string 'foo)").unwrap(),
        Value::string("foo")
    );
}

#[test]
fn test_symbol_to_string_type_error() {
    let result = eval_source("(symbol->string 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

// === Path primitives (path/* API) ===

#[test]
fn test_path_cwd() {
    let result = eval_source("(path/cwd)").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be a string");
    assert!(!s.is_empty());
}

#[test]
fn test_path_join() {
    let result = eval_source("(path/join \"a\" \"b\" \"c\")").unwrap();
    assert_eq!(result, Value::string("a/b/c"));
}

#[test]
fn test_path_join_single() {
    assert_eq!(
        eval_source("(path/join \"hello\")").unwrap(),
        Value::string("hello"),
    );
}

#[test]
fn test_path_join_type_error() {
    let result = eval_source("(path/join 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

#[test]
fn test_path_join_absolute_replaces() {
    assert_eq!(
        eval_source("(path/join \"a\" \"/b\")").unwrap(),
        Value::string("/b"),
    );
}

#[test]
fn test_path_parent() {
    assert_eq!(
        eval_source("(path/parent \"/home/user/data.txt\")").unwrap(),
        Value::string("/home/user"),
    );
}

#[test]
fn test_path_parent_root() {
    assert_eq!(eval_source("(path/parent \"/\")").unwrap(), Value::NIL);
}

#[test]
fn test_path_parent_relative() {
    assert_eq!(
        eval_source("(path/parent \"a/b/c\")").unwrap(),
        Value::string("a/b"),
    );
}

#[test]
fn test_path_filename() {
    assert_eq!(
        eval_source("(path/filename \"/home/user/data.txt\")").unwrap(),
        Value::string("data.txt"),
    );
}

#[test]
fn test_path_filename_bare() {
    assert_eq!(
        eval_source("(path/filename \"data.txt\")").unwrap(),
        Value::string("data.txt"),
    );
}

#[test]
fn test_path_filename_trailing_slash() {
    assert_eq!(
        eval_source("(path/filename \"/home/user/\")").unwrap(),
        Value::string("user"),
    );
}

#[test]
fn test_path_stem() {
    assert_eq!(
        eval_source("(path/stem \"data.txt\")").unwrap(),
        Value::string("data"),
    );
}

#[test]
fn test_path_stem_multiple_dots() {
    assert_eq!(
        eval_source("(path/stem \"archive.tar.gz\")").unwrap(),
        Value::string("archive.tar"),
    );
}

#[test]
fn test_path_extension() {
    assert_eq!(
        eval_source("(path/extension \"data.txt\")").unwrap(),
        Value::string("txt"),
    );
}

#[test]
fn test_path_extension_none() {
    assert_eq!(
        eval_source("(path/extension \"noext\")").unwrap(),
        Value::NIL
    );
}

#[test]
fn test_path_extension_multiple_dots() {
    assert_eq!(
        eval_source("(path/extension \"archive.tar.gz\")").unwrap(),
        Value::string("gz"),
    );
}

#[test]
fn test_path_with_extension() {
    assert_eq!(
        eval_source("(path/with-extension \"foo.txt\" \"rs\")").unwrap(),
        Value::string("foo.rs"),
    );
}

#[test]
fn test_path_normalize() {
    assert_eq!(
        eval_source("(path/normalize \"./a/../b\")").unwrap(),
        Value::string("b"),
    );
}

#[test]
fn test_path_absolute() {
    let result = eval_source("(path/absolute \"src\")").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be string");
    assert!(s.starts_with('/'), "absolute path should start with /");
}

#[test]
fn test_path_canonicalize_dot() {
    let result = eval_source("(path/canonicalize \".\")").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be string");
    assert!(s.starts_with('/'));
}

#[test]
fn test_path_canonicalize_nonexistent() {
    let result = eval_source("(path/canonicalize \"/nonexistent/path/xyz\")");
    assert!(result.is_err());
}

#[test]
fn test_path_relative() {
    assert_eq!(
        eval_source("(path/relative \"/foo/bar/baz\" \"/foo/bar\")").unwrap(),
        Value::string("baz"),
    );
}

#[test]
fn test_path_components() {
    // Components of "/a/b/c" should be a list: ("/" "a" "b" "c")
    let result = eval_source("(length (path/components \"/a/b/c\"))").unwrap();
    assert_eq!(result, Value::int(4));
}

#[test]
fn test_path_is_absolute() {
    assert_eq!(
        eval_source("(path/absolute? \"/foo\")").unwrap(),
        Value::TRUE
    );
    assert_eq!(
        eval_source("(path/absolute? \"foo\")").unwrap(),
        Value::FALSE
    );
}

#[test]
fn test_path_is_relative() {
    assert_eq!(
        eval_source("(path/relative? \"foo\")").unwrap(),
        Value::TRUE
    );
    assert_eq!(
        eval_source("(path/relative? \"/foo\")").unwrap(),
        Value::FALSE
    );
}

#[test]
fn test_path_exists() {
    assert_eq!(eval_source("(path/exists? \".\")").unwrap(), Value::TRUE);
    assert_eq!(
        eval_source("(path/exists? \"/nonexistent/xyz\")").unwrap(),
        Value::FALSE,
    );
}

#[test]
fn test_path_is_file() {
    assert_eq!(
        eval_source("(path/file? \"Cargo.toml\")").unwrap(),
        Value::TRUE
    );
    assert_eq!(eval_source("(path/file? \".\")").unwrap(), Value::FALSE);
}

#[test]
fn test_path_is_dir() {
    assert_eq!(eval_source("(path/dir? \".\")").unwrap(), Value::TRUE);
    assert_eq!(
        eval_source("(path/dir? \"Cargo.toml\")").unwrap(),
        Value::FALSE
    );
}

// --- Alias tests for predicates ---

#[test]
fn test_path_exists_alias_file_exists() {
    assert_eq!(eval_source("(file-exists? \".\")").unwrap(), Value::TRUE);
}

#[test]
fn test_path_dir_alias_directory() {
    assert_eq!(eval_source("(directory? \".\")").unwrap(), Value::TRUE);
}

#[test]
fn test_path_file_alias() {
    assert_eq!(eval_source("(file? \"Cargo.toml\")").unwrap(), Value::TRUE);
}

// === Read edge cases ===

#[test]
fn test_read_keyword() {
    let result = eval_source("(read \":hello\")").unwrap();
    assert_eq!(result.as_keyword_name().unwrap(), "hello");
}

#[test]
fn test_read_float() {
    assert_eq!(eval_source("(read \"2.5\")").unwrap(), Value::float(2.5));
}

#[test]
fn test_read_nil() {
    assert_eq!(eval_source("(read \"nil\")").unwrap(), Value::NIL);
}

#[test]
fn test_read_parse_error() {
    // Unbalanced parens should produce a read error
    let result = eval_source("(read \"(+ 1\")");
    assert!(result.is_err());
}

// === Conversion edge cases ===

#[test]
fn test_integer_zero() {
    assert_eq!(eval_source("(integer 0)").unwrap(), Value::int(0));
}

#[test]
fn test_integer_negative() {
    assert_eq!(eval_source("(integer -42)").unwrap(), Value::int(-42));
}

#[test]
fn test_float_zero() {
    assert_eq!(eval_source("(float 0)").unwrap(), Value::float(0.0));
}

#[test]
fn test_string_from_keyword() {
    let result = eval_source("(string :hello)").unwrap();
    let s = result
        .with_string(|s| s.to_string())
        .expect("should be string");
    assert_eq!(s, ":hello");
}

#[test]
fn test_string_from_empty_list() {
    // Empty list should have some string representation
    let result = eval_source("(string (list))").unwrap();
    assert!(result.is_string());
}

// === Alias tests ===

#[test]
fn test_string_to_int_alias() {
    // string->int is an alias for string->integer
    assert_eq!(eval_source("(string->int \"42\")").unwrap(), Value::int(42));
}

#[test]
fn test_int_alias() {
    // int is an alias for integer
    assert_eq!(eval_source("(int 42)").unwrap(), Value::int(42));
}

// === Type predicates for collections ===

#[test]
fn test_array_predicate_true() {
    assert_eq!(eval_source("(array? @[1 2 3])").unwrap(), Value::TRUE);
}

#[test]
fn test_array_predicate_false_tuple() {
    assert_eq!(eval_source("(array? [1 2 3])").unwrap(), Value::FALSE);
}

#[test]
fn test_array_predicate_false_other() {
    assert_eq!(eval_source("(array? 42)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(array? \"hello\")").unwrap(), Value::FALSE);
}

#[test]
fn test_tuple_predicate_true() {
    assert_eq!(eval_source("(tuple? [1 2 3])").unwrap(), Value::TRUE);
}

#[test]
fn test_tuple_predicate_false_array() {
    assert_eq!(eval_source("(tuple? @[1 2 3])").unwrap(), Value::FALSE);
}

#[test]
fn test_tuple_predicate_false_other() {
    assert_eq!(eval_source("(tuple? 42)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(tuple? \"hello\")").unwrap(), Value::FALSE);
}

#[test]
fn test_table_predicate_true() {
    assert_eq!(eval_source("(table? @{:a 1 :b 2})").unwrap(), Value::TRUE);
}

#[test]
fn test_table_predicate_false_struct() {
    assert_eq!(eval_source("(table? {:a 1 :b 2})").unwrap(), Value::FALSE);
}

#[test]
fn test_table_predicate_false_other() {
    assert_eq!(eval_source("(table? 42)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(table? \"hello\")").unwrap(), Value::FALSE);
}

#[test]
fn test_struct_predicate_true() {
    assert_eq!(eval_source("(struct? {:a 1 :b 2})").unwrap(), Value::TRUE);
}

#[test]
fn test_struct_predicate_false_table() {
    assert_eq!(eval_source("(struct? @{:a 1 :b 2})").unwrap(), Value::FALSE);
}

#[test]
fn test_struct_predicate_false_other() {
    assert_eq!(eval_source("(struct? 42)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(struct? \"hello\")").unwrap(), Value::FALSE);
}

#[test]
fn test_empty_predicate_tuple() {
    assert_eq!(eval_source("(empty? [])").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(empty? [1])").unwrap(), Value::FALSE);
}

#[test]
fn test_empty_predicate_array() {
    assert_eq!(eval_source("(empty? @[])").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(empty? @[1])").unwrap(), Value::FALSE);
}

// === fn/errors? introspection ===

#[test]
fn test_fn_errors_on_pure_closure() {
    // Identity function cannot signal an error
    assert_eq!(
        eval_source("(fn/errors? (fn (x) x))").unwrap(),
        Value::FALSE
    );
}

#[test]
fn test_fn_errors_on_non_closure() {
    // Non-closure values return false
    assert_eq!(eval_source("(fn/errors? 42)").unwrap(), Value::FALSE);
    assert_eq!(eval_source("(fn/errors? \"hello\")").unwrap(), Value::FALSE);
}

// === take/drop negative count (#434) ===

#[test]
fn test_take_negative_count_errors() {
    let result = eval_source("(take -1 (list 1 2 3))");
    assert!(result.is_err(), "take with negative count should error");
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_drop_negative_count_errors() {
    let result = eval_source("(drop -1 (list 1 2 3))");
    assert!(result.is_err(), "drop with negative count should error");
    assert!(result.unwrap_err().contains("non-negative"));
}

#[test]
fn test_take_zero() {
    assert_eq!(
        eval_source("(take 0 (list 1 2 3))").unwrap(),
        Value::EMPTY_LIST
    );
}

#[test]
fn test_drop_zero() {
    assert_eq!(
        eval_source("(drop 0 (list 1 2 3))").unwrap(),
        eval_source("(list 1 2 3)").unwrap()
    );
}

// === Bitwise float truncation (#432) ===

#[test]
fn test_bit_and_float_truncation() {
    assert_eq!(eval_source("(bit/and 12.7 10.3)").unwrap(), Value::int(8));
}

#[test]
fn test_bit_or_float_truncation() {
    assert_eq!(eval_source("(bit/or 12.7 10.3)").unwrap(), Value::int(14));
}

#[test]
fn test_bit_xor_float_truncation() {
    assert_eq!(eval_source("(bit/xor 12.7 10.3)").unwrap(), Value::int(6));
}

#[test]
fn test_bit_not_float_truncation() {
    // 0.9 truncates to 0, bit/not 0 = -1
    assert_eq!(eval_source("(bit/not 0.9)").unwrap(), Value::int(-1));
}

#[test]
fn test_bit_shl_float_value() {
    // 1.9 truncates to 1, shift left 3 = 8
    assert_eq!(eval_source("(bit/shl 1.9 3)").unwrap(), Value::int(8));
}

#[test]
fn test_bit_shr_float_value() {
    // 8.7 truncates to 8, shift right 2 = 2
    assert_eq!(eval_source("(bit/shr 8.7 2)").unwrap(), Value::int(2));
}

#[test]
fn test_bit_and_nan_errors() {
    let result = eval_source("(bit/and (sqrt -1.0) 1)");
    assert!(result.is_err(), "NaN should error in bitwise ops");
    assert!(result.unwrap_err().contains("non-finite"));
}

#[test]
fn test_bit_and_inf_errors() {
    let result = eval_source("(bit/and (exp 1000.0) 1)");
    assert!(result.is_err(), "infinity should error in bitwise ops");
    assert!(result.unwrap_err().contains("non-finite"));
}

#[test]
fn test_bit_and_neg_float() {
    // -3.7 truncates to -3
    assert_eq!(
        eval_source("(bit/and -3.7 255)").unwrap(),
        eval_source("(bit/and -3 255)").unwrap()
    );
}

// === first polymorphism (#423) ===

#[test]
fn test_first_list() {
    assert_eq!(eval_source("(first (list 1 2 3))").unwrap(), Value::int(1));
}

#[test]
fn test_first_empty_list() {
    assert_eq!(eval_source("(first (list))").unwrap(), Value::NIL);
}

#[test]
fn test_first_tuple() {
    assert_eq!(eval_source("(first [1 2 3])").unwrap(), Value::int(1));
}

#[test]
fn test_first_empty_tuple() {
    assert_eq!(eval_source("(first [])").unwrap(), Value::NIL);
}

#[test]
fn test_first_array() {
    assert_eq!(eval_source("(first @[1 2 3])").unwrap(), Value::int(1));
}

#[test]
fn test_first_empty_array() {
    assert_eq!(eval_source("(first @[])").unwrap(), Value::NIL);
}

#[test]
fn test_first_string() {
    assert_eq!(eval_source(r#"(first "abc")"#).unwrap(), Value::string("a"));
}

#[test]
fn test_first_empty_string() {
    assert_eq!(eval_source(r#"(first "")"#).unwrap(), Value::NIL);
}

#[test]
fn test_first_non_sequence_errors() {
    let result = eval_source("(first 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

// === rest polymorphism (#423) ===

#[test]
fn test_rest_list() {
    // rest of (1 2 3) is (2 3)
    assert_eq!(
        eval_source("(first (rest (list 1 2 3)))").unwrap(),
        Value::int(2)
    );
}

#[test]
fn test_rest_empty_list() {
    assert_eq!(eval_source("(rest (list))").unwrap(), Value::EMPTY_LIST);
}

#[test]
fn test_rest_single_list() {
    assert_eq!(eval_source("(rest (list 1))").unwrap(), Value::EMPTY_LIST);
}

#[test]
fn test_rest_tuple() {
    // rest of [1 2 3] is [2 3] — a tuple
    assert_eq!(
        eval_source("(length (rest [1 2 3]))").unwrap(),
        Value::int(2)
    );
    // verify it's a tuple by checking tuple?
    assert_eq!(eval_source("(tuple? (rest [1 2 3]))").unwrap(), Value::TRUE);
}

#[test]
fn test_rest_empty_tuple() {
    assert_eq!(eval_source("(tuple? (rest []))").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(length (rest []))").unwrap(), Value::int(0));
}

#[test]
fn test_rest_array() {
    // rest of @[1 2 3] is @[2 3] — an array
    assert_eq!(
        eval_source("(length (rest @[1 2 3]))").unwrap(),
        Value::int(2)
    );
    assert_eq!(
        eval_source("(array? (rest @[1 2 3]))").unwrap(),
        Value::TRUE
    );
}

#[test]
fn test_rest_empty_array() {
    assert_eq!(eval_source("(array? (rest @[]))").unwrap(), Value::TRUE);
    assert_eq!(eval_source("(length (rest @[]))").unwrap(), Value::int(0));
}

#[test]
fn test_rest_string() {
    assert_eq!(eval_source(r#"(rest "abc")"#).unwrap(), Value::string("bc"));
}

#[test]
fn test_rest_empty_string() {
    assert_eq!(eval_source(r#"(rest "")"#).unwrap(), Value::string(""));
}

#[test]
fn test_rest_single_string() {
    assert_eq!(eval_source(r#"(rest "a")"#).unwrap(), Value::string(""));
}

#[test]
fn test_rest_non_sequence_errors() {
    let result = eval_source("(rest 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}

// === reverse polymorphism (#431) ===

#[test]
fn test_reverse_list() {
    assert_eq!(
        eval_source("(first (reverse (list 1 2 3)))").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_reverse_empty_list() {
    assert_eq!(eval_source("(reverse (list))").unwrap(), Value::EMPTY_LIST);
}

#[test]
fn test_reverse_tuple() {
    assert_eq!(
        eval_source("(tuple? (reverse [1 2 3]))").unwrap(),
        Value::TRUE
    );
    assert_eq!(
        eval_source("(get (reverse [1 2 3]) 0)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_reverse_empty_tuple() {
    assert_eq!(eval_source("(tuple? (reverse []))").unwrap(), Value::TRUE);
}

#[test]
fn test_reverse_array() {
    assert_eq!(
        eval_source("(array? (reverse @[1 2 3]))").unwrap(),
        Value::TRUE
    );
    assert_eq!(
        eval_source("(get (reverse @[1 2 3]) 0)").unwrap(),
        Value::int(3)
    );
}

#[test]
fn test_reverse_empty_array() {
    assert_eq!(eval_source("(array? (reverse @[]))").unwrap(), Value::TRUE);
}

#[test]
fn test_reverse_string() {
    assert_eq!(
        eval_source(r#"(reverse "abc")"#).unwrap(),
        Value::string("cba")
    );
}

#[test]
fn test_reverse_empty_string() {
    assert_eq!(eval_source(r#"(reverse "")"#).unwrap(), Value::string(""));
}

#[test]
fn test_reverse_non_sequence_errors() {
    let result = eval_source("(reverse 42)");
    assert!(result.is_err());
    assert!(result.unwrap_err().contains("type"));
}
