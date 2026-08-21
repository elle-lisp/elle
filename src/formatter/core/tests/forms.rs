use super::*;

// ── Idempotency tests for each special form ─────────────────

#[test]
fn test_idempotent_if_with_else() {
    let config = FormatterConfig::default();
    let input = "(if true 1 2)";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_if_complex() {
    let config = FormatterConfig::default();
    let input = "(if (< x 10) (print x) (print (- x 10)))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_fn_single_body() {
    let config = FormatterConfig::default();
    let input = "(fn (x) (+ x 1))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_fn_multi_body() {
    let config = FormatterConfig::default();
    let input = "(fn (x) (print x) (+ x 1))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_begin() {
    let config = FormatterConfig::default();
    let input = "(begin (print 1) (print 2) (print 3))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_when() {
    let config = FormatterConfig::default();
    let input = "(when (> x 0) (print x) x)";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_cond() {
    let config = FormatterConfig::default();
    let input = "(cond (< x 0) \"neg\" (= x 0) \"zero\" true \"pos\")";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_match() {
    let config = FormatterConfig::default();
    let input = "(match x 1 \"one\" 2 \"two\" _ \"other\")";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_threading() {
    let config = FormatterConfig::default();
    let input = "(-> x (f 1) (g 2) (h 3))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_each() {
    let config = FormatterConfig::default();
    let input = "(each item in items (print item))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_and_or() {
    let config = FormatterConfig::default();
    let first_and = format_code("(and a b c)", &config).unwrap();
    let second_and = format_code(&first_and, &config).unwrap();
    assert_eq!(first_and, second_and);

    let first_or = format_code("(or x y z)", &config).unwrap();
    let second_or = format_code(&first_or, &config).unwrap();
    assert_eq!(first_or, second_or);
}

#[test]
fn test_idempotent_defmacro() {
    let config = FormatterConfig::default();
    let input = "(defmacro swap (a b) `(let [tmp ,a] (assign ,a ,b) (assign ,b tmp)))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_assign() {
    let config = FormatterConfig::default();
    let input = "(assign x 42)";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

#[test]
fn test_idempotent_def() {
    let config = FormatterConfig::default();
    let input = "(def my-fn (fn (x) (+ x 1)))";
    let first = format_code(input, &config).unwrap();
    let second = format_code(&first, &config).unwrap();
    assert_eq!(first, second);
}

// ── Collection type tests ──────────────────────────────────

#[test]
fn test_format_array() {
    let config = FormatterConfig::default();
    let formatted = format_code("[1 2 3]", &config).unwrap();
    assert_eq!(formatted, "[1 2 3]\n");
}

#[test]
fn test_format_set() {
    let config = FormatterConfig::default();
    let formatted = format_code("|a b c|", &config).unwrap();
    assert_eq!(formatted, "|a b c|\n");
}

#[test]
fn test_format_struct() {
    let config = FormatterConfig::default();
    let formatted = format_code("{:x 1 :y 2}", &config).unwrap();
    assert_eq!(formatted, "{:x 1 :y 2}\n");
}

#[test]
fn test_format_nested_quote() {
    let config = FormatterConfig::default();
    assert_eq!(format_code("'foo", &config).unwrap(), "'foo\n");
    assert_eq!(format_code("'(1 2 3)", &config).unwrap(), "'(1 2 3)\n");
}

#[test]
fn test_format_quasiquote() {
    let config = FormatterConfig::default();
    let formatted = format_code("`(foo ,bar ;baz)", &config).unwrap();
    let second = format_code(&formatted, &config).unwrap();
    assert_eq!(formatted, second);
}
