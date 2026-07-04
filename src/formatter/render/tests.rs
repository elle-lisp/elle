//! Unit tests (`super` is the parent impl module).

use super::*;

fn default_config() -> FormatterConfig {
    FormatterConfig::default()
}

#[test]
fn test_empty() {
    assert_eq!(render(&Doc::empty(), &default_config()), "");
}

#[test]
fn test_text() {
    assert_eq!(render(&Doc::text("hello"), &default_config()), "hello");
}

#[test]
fn test_concat_texts() {
    let doc = Doc::concat([Doc::text("hello"), Doc::text(" world")]);
    assert_eq!(render(&doc, &default_config()), "hello world");
}

#[test]
fn test_group_fits() {
    let doc = Doc::concat([
        Doc::text("("),
        Doc::concat([Doc::text("a"), Doc::Break, Doc::text("b")]).group(),
        Doc::text(")"),
    ]);
    assert_eq!(render(&doc, &default_config()), "(a b)");
}

#[test]
fn test_group_breaks() {
    let config = FormatterConfig::new().with_line_length(10);
    let doc = Doc::concat([
        Doc::text("("),
        Doc::concat([Doc::text("hello"), Doc::Break, Doc::text("world")])
            .nest(1)
            .group(),
        Doc::text(")"),
    ]);
    let result = render(&doc, &config);
    assert!(result.contains('\n'), "should break: got {:?}", result);
    let lines: Vec<&str> = result.lines().collect();
    assert!(
        lines[1].starts_with("  "),
        "second line should be indented: {:?}",
        lines
    );
    assert!(
        lines[1].contains("world"),
        "second line should contain 'world': {:?}",
        lines
    );
}

#[test]
fn test_nest_indentation_with_leading_break() {
    let config = FormatterConfig::new()
        .with_line_length(10)
        .with_indent_width(2);
    let doc = Doc::concat([
        Doc::text("("),
        Doc::concat([
            Doc::HardBreak,
            Doc::text("a"),
            Doc::HardBreak,
            Doc::text("b"),
        ])
        .nest(1),
        Doc::HardBreak,
        Doc::text(")"),
    ]);
    let result = render(&doc, &config);
    assert_eq!(result, "(\n  a\n  b\n)", "got: {:?}", result);
}

#[test]
fn test_nest_indentation_same_line() {
    let config = FormatterConfig::new()
        .with_line_length(10)
        .with_indent_width(2);
    let doc = Doc::concat([
        Doc::text("("),
        Doc::concat([Doc::text("a"), Doc::HardBreak, Doc::text("b")]).nest(1),
        Doc::HardBreak,
        Doc::text(")"),
    ]);
    let result = render(&doc, &config);
    assert_eq!(result, "(a\n  b\n)", "got: {:?}", result);
}

#[test]
fn test_hardbreak_alone() {
    let doc = Doc::concat([Doc::text("a"), Doc::HardBreak, Doc::text("b")]);
    assert_eq!(render(&doc, &default_config()), "a\nb");
}

#[test]
fn test_hardbreak_forces_group_break() {
    let doc = Doc::concat([Doc::text("a"), Doc::HardBreak, Doc::text("b")]).group();
    let result = render(&doc, &default_config());
    assert_eq!(result, "a\nb", "got: {:?}", result);
}

#[test]
fn test_nested_group_outer_fits_inner_also_fits() {
    let config = FormatterConfig::new().with_line_length(40);
    let doc = Doc::concat([
        Doc::text("outer-start "),
        Doc::concat([Doc::text("inner-a"), Doc::Break, Doc::text("inner-b")]).group(),
    ])
    .group();
    let result = render(&doc, &config);
    assert_eq!(result, "outer-start inner-a inner-b");
}

#[test]
fn test_nested_group_outer_breaks_inner_also_breaks() {
    let config = FormatterConfig::new().with_line_length(10);
    let doc = Doc::concat([
        Doc::text("start"),
        Doc::Break,
        Doc::concat([Doc::text("inner-a"), Doc::Break, Doc::text("inner-b")])
            .nest(1)
            .group(),
    ])
    .nest(1)
    .group();
    let result = render(&doc, &config);
    assert!(result.contains('\n'), "should break: got {:?}", result);
}

#[test]
fn test_list_formatting_short() {
    let doc = Doc::concat([
        Doc::text("("),
        Doc::intersperse([Doc::text("a"), Doc::text("b"), Doc::text("c")]).group(),
        Doc::text(")"),
    ]);
    assert_eq!(render(&doc, &default_config()), "(a b c)");
}

#[test]
fn test_list_formatting_long() {
    let config = FormatterConfig::new().with_line_length(20);
    let doc = Doc::concat([
        Doc::text("("),
        Doc::intersperse([Doc::text("long-argument-1"), Doc::text("long-argument-2")])
            .nest(1)
            .group(),
        Doc::text(")"),
    ]);
    let result = render(&doc, &config);
    assert!(result.contains('\n'), "should break: got {:?}", result);
    let lines: Vec<&str> = result.lines().collect();
    assert!(lines.len() >= 2, "expected 2+ lines, got: {:?}", lines);
    assert!(
        lines[0].starts_with("(long-argument-1"),
        "first line should start with (long-argument-1: {:?}",
        lines
    );
    assert!(
        lines[1].starts_with("  long-argument-2"),
        "second line should be indented: {:?}",
        lines
    );
}

#[test]
fn test_measure_flat_values() {
    assert_eq!(measure_flat(&Doc::empty()), Some(Column::ZERO));
    assert_eq!(measure_flat(&Doc::text("hello")), Some(Column::new(5)));
    assert_eq!(measure_flat(&Doc::Break), Some(Column::new(1)));
    assert_eq!(measure_flat(&Doc::HardBreak), None);
    assert_eq!(
        measure_flat(&Doc::concat([Doc::text("ab"), Doc::Break, Doc::text("cd")])),
        Some(Column::new(5))
    );
    assert_eq!(
        measure_flat(&Doc::concat([
            Doc::text("a"),
            Doc::HardBreak,
            Doc::text("b")
        ])),
        None
    );
}

#[test]
fn test_deeply_nested_indentation() {
    let config = FormatterConfig::new().with_indent_width(2);
    let doc = Doc::concat([
        Doc::text("outer"),
        Doc::concat([
            Doc::HardBreak,
            Doc::text("mid"),
            Doc::concat([Doc::HardBreak, Doc::text("inner")]).nest(1),
        ])
        .nest(1),
    ]);
    let result = render(&doc, &config);
    let expected = "outer\n  mid\n    inner";
    assert_eq!(result, expected, "got: {:?}", result);
}

#[test]
fn test_nest_only_affects_breaks() {
    let config = FormatterConfig::new().with_indent_width(2);
    let doc = Doc::concat([
        Doc::text("a"),
        Doc::concat([Doc::text("b"), Doc::HardBreak, Doc::text("c")]).nest(1),
    ]);
    let result = render(&doc, &config);
    assert_eq!(result, "ab\n  c", "got: {:?}", result);
}

#[test]
fn test_align() {
    let config = FormatterConfig::new().with_line_length(15);
    let doc = Doc::concat([
        Doc::text("(foo "),
        Doc::align(
            Doc::concat([
                Doc::text("long-a"),
                Doc::Break,
                Doc::text("long-b"),
                Doc::Break,
                Doc::text("long-c"),
            ])
            .group(),
        ),
        Doc::text(")"),
    ]);
    let result = render(&doc, &config);
    // When broken, long-b and long-c align to column 5 (after "(foo ")
    assert_eq!(
        result, "(foo long-a\n     long-b\n     long-c)",
        "got: {:?}",
        result
    );
}
