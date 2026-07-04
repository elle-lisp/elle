use super::*;

// ── CommentBreak idempotency tests ──────────────────────────

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

// ── Own-line comments are PRESERVED as leading trivia, not deleted ──────
//
// The attachment pass used to collect every trivia item between two children as
// *trailing* of the preceding child; a form handler that formats a header child
// specially and drops its trailing trivia (format_let, format_parameterize) then
// deleted the mis-attached comment outright — `elle fmt` stripped comments out of
// source (e.g. lib/process.lisp). The fix: an own-line comment is *leading* of the
// following child (same-line comments stay inline-trailing), and format_let /
// format_parameterize emit the bindings vector's own trailing trivia.
//
// Idempotency alone does NOT catch this — a formatter that deleted/relocated
// comments would do so *idempotently*. So these assert the comment SURVIVES and
// lands on the correct line.

/// Format `input`, assert idempotency, and return the output for content checks.
fn format_preserving(input: &str) -> String {
    let config = FormatterConfig::default();
    let out = format_code(input, &config).unwrap();
    let again = format_code(&out, &config).unwrap();
    assert_eq!(out, again, "comment-preserving format must be idempotent");
    out
}

#[test]
fn test_comment_preserved_in_let_body() {
    let out = format_preserving("(let [x 1]\n  # leading comment\n  (+ x 1))");
    assert!(
        out.contains("]\n  # leading comment"),
        "own-line comment must stay on its own line in a let body, got:\n{}",
        out
    );
}

#[test]
fn test_comment_preserved_in_fn_body_first() {
    let out = format_preserving("(defn f []\n  # first body comment\n  (+ 1 2))");
    assert!(
        out.contains("]\n  # first body comment"),
        "first body comment must stay a leading comment on its own line, got:\n{}",
        out
    );
}

#[test]
fn test_comment_preserved_stacked_in_begin() {
    let out = format_preserving("(begin\n  # stacked one\n  # stacked two\n  (a)\n  (b))");
    assert!(
        out.contains("# stacked one\n  # stacked two"),
        "stacked own-line comments must both survive, in order, got:\n{}",
        out
    );
}

#[test]
fn test_comment_between_body_forms_own_line() {
    let out = format_preserving("(begin\n  (a)\n  # between the forms\n  (b))");
    assert!(
        out.contains("(a)\n  # between the forms\n  (b)"),
        "a comment between two body forms must stay on its own line, got:\n{}",
        out
    );
}

#[test]
fn test_comment_preserved_inline_after_let_bindings() {
    let out = format_preserving("(let [a 1\n      b 2]  # trailing bindings\n  (+ a b))");
    assert!(
        out.contains("]  # trailing bindings"),
        "a same-line comment after the bindings vector must stay inline, got:\n{}",
        out
    );
}

#[test]
fn test_comment_preserved_inline_after_parameterize_bindings() {
    let out = format_preserving("(parameterize ((*x* 1))  # trailing params\n  (body))");
    assert!(
        out.contains(")  # trailing params"),
        "a same-line comment after the parameterize bindings must stay inline, got:\n{}",
        out
    );
}
