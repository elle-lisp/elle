use super::*;

#[test]
fn test_format_simple_number() {
    let config = FormatterConfig::default();
    let formatted = format_code("42", &config).unwrap();
    assert_eq!(formatted, "42\n");
}

#[test]
fn test_format_simple_list() {
    let config = FormatterConfig::default();
    let formatted = format_code("(+ 1 2)", &config).unwrap();
    assert!(formatted.contains('('));
    assert!(formatted.contains(')'));
}

#[test]
fn test_format_nil() {
    let config = FormatterConfig::default();
    let formatted = format_code("nil", &config).unwrap();
    assert_eq!(formatted, "nil\n");
}

#[test]
fn test_format_boolean() {
    let config = FormatterConfig::default();
    let formatted_true = format_code("true", &config).unwrap();
    let formatted_false = format_code("false", &config).unwrap();
    assert_eq!(formatted_true, "true\n");
    assert_eq!(formatted_false, "false\n");
}

#[test]
fn test_format_string() {
    let config = FormatterConfig::default();
    let formatted = format_code("\"hello\"", &config).unwrap();
    assert!(formatted.contains("hello"));
}

#[test]
fn test_format_vector() {
    let config = FormatterConfig::default();
    let formatted = format_code("[1 2 3]", &config).unwrap();
    assert!(formatted.contains('['));
    assert!(formatted.contains(']'));
}

#[test]
fn test_trailing_newline() {
    let config = FormatterConfig::default();
    let formatted = format_code("(+ 1 2)", &config).unwrap();
    assert!(formatted.ends_with('\n'), "must end with newline");
}

#[test]
fn test_empty_source() {
    let config = FormatterConfig::default();
    let formatted = format_code("", &config).unwrap();
    assert_eq!(formatted, "\n");
}

#[test]
fn test_multiple_forms() {
    let config = FormatterConfig::default();
    let formatted = format_code("(def x 5)\n(+ x 1)", &config).unwrap();
    let lines: Vec<&str> = formatted.trim_end().lines().collect();
    assert!(lines.len() >= 2, "should have 2+ lines: {:?}", lines);
}

#[test]
fn test_idempotent_simple() {
    let config = FormatterConfig::default();
    let first = format_code("(+ 1 2)", &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second, "formatter must be idempotent");
}

#[test]
fn test_idempotent_defn() {
    let config = FormatterConfig::default();
    let input = "(defn foo [x] (+ x 1))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second, "defn formatting must be idempotent");
}

#[test]
fn test_idempotent_let() {
    let config = FormatterConfig::default();
    let input = "(let [x 5] (+ x 1))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second, "let formatting must be idempotent");
}

#[test]
fn test_shebang_preserved() {
    let config = FormatterConfig::default();
    let input = "#!/usr/bin/env elle\n(+ 1 2)";
    let formatted = format_code(input, &config).unwrap();
    assert!(
        formatted.starts_with("#!/usr/bin/env elle\n"),
        "shebang must be preserved"
    );
}

#[test]
fn test_keyword() {
    let config = FormatterConfig::default();
    let formatted = format_code(":hello", &config).unwrap();
    assert_eq!(formatted, ":hello\n");
}

#[test]
fn test_set_literal() {
    let config = FormatterConfig::default();
    let formatted = format_code("|1 2 3|", &config).unwrap();
    assert!(formatted.contains('|'));
}

#[test]
fn test_quote() {
    let config = FormatterConfig::default();
    let formatted = format_code("'foo", &config).unwrap();
    assert_eq!(formatted, "'foo\n");
}

#[test]
fn test_nested_list() {
    let config = FormatterConfig::default();
    let formatted = format_code("(defn foo [x] (if (> x 0) x (- x)))", &config).unwrap();
    let second = format_code(&formatted, &config).unwrap();
    assert_eq!(formatted, second, "nested formatting must be idempotent");
}

#[test]
fn test_inspect_defn_output() {
    let config = FormatterConfig::default();
    let input = "(defn fib [n] (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))";
    let formatted = format_code(input, &config).unwrap();
    // defn always breaks before body
    assert!(formatted.contains('\n'), "defn should break before body");
    let lines: Vec<&str> = formatted.trim_end().lines().collect();
    assert!(
        lines[0].starts_with("(defn fib [n]"),
        "first line: {:?}",
        lines
    );
    let second = format_code(&formatted, &config).unwrap();
    assert_eq!(formatted, second);
}

#[test]
fn test_inspect_let_output() {
    let config = FormatterConfig::default();
    let input = "(let [x 5 y 10] (+ x y))";
    let formatted = format_code(input, &config).unwrap();
    // let with multiple pairs breaks between pairs
    assert!(
        formatted.contains("[x 5\n"),
        "let bindings should have pairs on separate lines: {:?}",
        formatted
    );
    let second = format_code(&formatted, &config).unwrap();
    assert_eq!(formatted, second, "let must be idempotent");
}

#[test]
fn test_inspect_full_file() {
    let config = FormatterConfig::default();
    let input = r#"(defn fib [n]
  (if (< n 2) n (+ (fib (- n 1)) (fib (- n 2)))))

(def x 5)

(let [a 1 b 2 c 3]
  (+ a b c))

(begin
  (print "hello")
  (print "world"))

(when (> x 0)
  (print "positive")
  x)

(cond
  (< n 0) "negative"
  (= n 0) "zero"
  true "positive")

(match x
  1 "one"
  2 "two"
  _ "other")

(-> val
  (f a)
  (g b))

(each item in items
  (print item))

(and a b c)

'foo
[1 2 3]
|a b c|
{:x 1 :y 2}
"hello world"
42
true
nil
:keyword"#;
    let formatted = format_code(input, &config).unwrap();
    let second = format_code(&formatted, &config).unwrap();
    assert_eq!(formatted, second, "full file must be idempotent");
}
