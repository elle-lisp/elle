//! Core formatting entry point.
//!
//! Implements the full formatting pipeline:
//!
//! ```text
//! Source → strip shebang → lex (separate tokens + comments)
//!       → parse to Syntax → collect trivia → attach trivia
//!       → generate Doc → render → prepend shebang + trailing newline
//! ```

use super::comments::{lex_for_format, strip_shebang};
use super::config::FormatterConfig;
use super::format::format_forms;
use super::render::render;
use super::trivia::{collect_trivia, AnnotatedSyntax};
use crate::reader::SyntaxReader;

/// Format Elle source code with the given configuration.
///
/// Returns the formatted string, or an error if parsing fails.
pub fn format_code(source: &str, config: &FormatterConfig) -> Result<String, String> {
    // 1. Strip shebang (single strip point for consistent byte offsets)
    let (stripped, shebang) = strip_shebang(source);

    // 2. Lex: separate regular tokens from comment tokens
    let lexed = lex_for_format(stripped, "<format>")?;

    // 3. Parse regular tokens to Syntax tree
    let forms = if lexed.tokens.is_empty() {
        Vec::new()
    } else {
        let mut parser = SyntaxReader::with_byte_offsets(
            lexed.tokens,
            lexed.locations,
            lexed.lengths,
            lexed.byte_offsets,
        );
        parser.read_all()?
    };

    // 4. Collect trivia: merge comments from lexer with blank lines from source
    let comment_data: Vec<(String, usize, u32)> = lexed
        .comment_map
        .comments()
        .iter()
        .map(|c| (c.text.clone(), c.byte_offset, c.line))
        .collect();
    let trivia = collect_trivia(stripped, &comment_data);

    // 5. Attach trivia to Syntax nodes
    let (annotated, dangling) = AnnotatedSyntax::build_toplevel(forms, &trivia, stripped);

    // 6. Generate Doc tree from annotated syntax
    let doc = format_forms(&annotated, &dangling, stripped, config);

    // 7. Render Doc to string
    let rendered = render(&doc, config);

    // 8. Assemble output: shebang + rendered + trailing newline
    //    Strip leading newline from rendered output — format_annotated
    //    emits HardBreak before leading comments, which produces a
    //    spurious newline at the document start.
    let rendered = rendered.trim_start_matches('\n');

    let mut output = String::new();
    if !shebang.is_empty() {
        output.push_str(shebang);
    }
    output.push_str(rendered);

    // 9. Strip trailing whitespace from every line. The renderer emits
    //    indent spaces before HardBreaks (blank separator lines), producing
    //    lines with only whitespace. Strip these so editors and linters
    //    don't complain.
    let output: String = output
        .lines()
        .map(|line| line.trim_end())
        .collect::<Vec<_>>()
        .join("\n");

    let mut output = output;
    if !output.ends_with('\n') {
        output.push('\n');
    }

    Ok(output)
}

#[cfg(test)]
mod tests {
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

    // ── CommentBreak idempotency tests ──────────────────────────

    fn assert_idempotent(input: &str) {
        let config = FormatterConfig::default();
        let first = format_code(input, &config).unwrap();
        let second = format_code(&first, &config).unwrap();
        assert_eq!(
            first, second,
            "not idempotent:\n--- first ---\n{}\n--- second ---\n{}",
            first, second
        );
    }

    #[test]
    fn test_idempotent_trailing_comment_non_last() {
        assert_idempotent("(begin\n  (foo)  # comment\n  (bar))");
    }

    #[test]
    fn test_idempotent_trailing_comment_last() {
        assert_idempotent("(begin\n  (foo)  # comment\n)");
    }

    #[test]
    fn test_idempotent_block_comment_between() {
        assert_idempotent("(begin\n  (foo)\n  # between\n  (bar))");
    }

    #[test]
    fn test_idempotent_block_comment_before_close() {
        assert_idempotent("(defn f [x]\n  # before close\n  x)");
    }

    #[test]
    fn test_idempotent_inline_comment_blank_line_next() {
        assert_idempotent("(foo)  # comment\n\n(bar)");
    }

    #[test]
    fn test_idempotent_nested_comments_multi_level() {
        assert_idempotent("(defn outer [x]\n  (let [a 1]  # bind\n    (inner a)))  # done");
    }

    #[test]
    fn test_idempotent_cond_trivial_and_compound() {
        assert_idempotent(
            "(cond (< x 0) \"neg\" (= x 0) (begin (print \"zero\") \"zero\") true \"pos\")",
        );
    }

    #[test]
    fn test_idempotent_case_trivial_and_compound() {
        assert_idempotent("(case x :a 1 :b (begin (print \"b\") 2) :c 3)");
    }

    #[test]
    fn test_idempotent_let_star() {
        assert_idempotent("(let* [x 5 y (+ x 1)] (+ x y))");
    }

    #[test]
    fn test_idempotent_when_with_comment() {
        assert_idempotent("(when (> x 0)  # guard\n  (print x))");
    }

    #[test]
    fn test_idempotent_defn_with_comments() {
        assert_idempotent("(defn foo [x]  # params\n  # body comment\n  (+ x 1))");
    }

    #[test]
    fn test_idempotent_generic_call_long_head() {
        assert_idempotent("(some-very-long-function-name arg1 arg2 arg3 arg4)");
    }

    #[test]
    fn test_idempotent_generic_call_short_head() {
        assert_idempotent("(f arg1 arg2 arg3)");
    }

    // ── Trailing trivia on params must not split header ──────────

    #[test]
    fn test_fn_header_not_split_by_trailing_comment() {
        let config = FormatterConfig::default();
        let input = "(fn [x y]\n\n  ## doc comment\n  (+ x y))";
        let formatted = format_code(input, &config).unwrap();
        assert!(
            formatted.starts_with("(fn [x y]"),
            "fn header should stay on one line, got:\n{}",
            formatted
        );
        let second = format_code(&formatted, &config).unwrap();
        assert_eq!(formatted, second, "must be idempotent");
    }

    #[test]
    fn test_defn_header_not_split_by_trailing_comment() {
        let config = FormatterConfig::default();
        let input = "(defn foo [x y]\n\n  ## doc comment\n  (+ x y))";
        let formatted = format_code(input, &config).unwrap();
        assert!(
            formatted.starts_with("(defn foo [x y]"),
            "defn header should stay on one line, got:\n{}",
            formatted
        );
        let second = format_code(&formatted, &config).unwrap();
        assert_eq!(formatted, second, "must be idempotent");
    }

    #[test]
    fn test_no_trailing_whitespace() {
        let config = FormatterConfig::default();
        let input = "(fn [x]\n\n  ## comment\n  (+ x 1))";
        let formatted = format_code(input, &config).unwrap();
        for (i, line) in formatted.lines().enumerate() {
            assert!(
                line == line.trim_end(),
                "line {} has trailing whitespace: {:?}",
                i + 1,
                line
            );
        }
    }

    #[test]
    fn test_if_branches_align_in_let_binding() {
        let config = FormatterConfig::default();
        // When (if ...) is a value in a let binding and breaks, branches
        // must align relative to the (if column, not the ambient nest.
        let input = "(let [port (if (nil? colon) info:default-port (parse-int (slice auth (inc colon))))] port)";
        let formatted = format_code(input, &config).unwrap();
        let second = format_code(&formatted, &config).unwrap();
        assert_eq!(formatted, second, "must be idempotent");
        // The branches should be indented relative to the ( of (if,
        // not at some unrelated nest level.
        let lines: Vec<&str> = formatted.lines().collect();
        // Find the line with (if
        let if_line_idx = lines.iter().position(|l| l.contains("(if")).unwrap();
        let if_col = lines[if_line_idx].find("(if").unwrap();
        // Branch lines should be at if_col + 2 (standard body indent from "(")
        if if_line_idx + 1 < lines.len() {
            let branch_line = lines[if_line_idx + 1];
            let branch_col = branch_line.len() - branch_line.trim_start().len();
            assert_eq!(
                branch_col,
                if_col + 2,
                "branch should indent +2 from (if at col {}, got col {}\nformatted:\n{}",
                if_col,
                branch_col,
                formatted
            );
        }
    }

    #[test]
    fn test_fn_named_args_header_not_split() {
        let config = FormatterConfig::default();
        let input = "(fn [&named tls compress]\n\n  ## comment block\n  (def x 1)\n  (def y 2))";
        let formatted = format_code(input, &config).unwrap();
        assert!(
            formatted.starts_with("(fn [&named tls compress]"),
            "fn &named header should stay on one line, got:\n{}",
            formatted
        );
        let second = format_code(&formatted, &config).unwrap();
        assert_eq!(formatted, second, "must be idempotent");
    }
}
