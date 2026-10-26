// audited: 2026-10-25
//! Tests of the trivia layer: collecting comments and blank lines, and hanging
//! each on a node.
//!
//! src/formatter/README.md

use super::*;

#[test]
fn test_collect_trivia_empty() {
    let trivia = collect_trivia("", &[]);
    assert!(trivia.is_empty());
}

#[test]
fn test_collect_trivia_no_trivia() {
    let trivia = collect_trivia("(+ 1 2)", &[]);
    assert!(trivia.is_empty());
}

#[test]
fn test_collect_trivia_comment() {
    let comments = vec![CommentInfo {
        text: "# hello".to_string(),
        offset: ByteOffset::new(0),
        line: LineNum::new(1),
    }];
    let trivia = collect_trivia("# hello\n(+ 1 2)", &comments);
    assert_eq!(trivia.len(), 1);
    assert!(matches!(&trivia[0], Trivia::Comment { text, .. } if text == "# hello"));
}

#[test]
fn test_collect_trivia_blank_lines() {
    let trivia = collect_trivia("a\n\n\nb", &[]);
    assert_eq!(trivia.len(), 1);
    assert!(matches!(&trivia[0], Trivia::BlankLines { count, .. } if count.get() == 2));
}

#[test]
fn test_collect_trivia_sorted() {
    let comments = vec![CommentInfo {
        text: "# second".to_string(),
        offset: ByteOffset::new(5),
        line: LineNum::new(2),
    }];
    let source = "# first\n# second\n42";
    let mut trivia = collect_trivia(source, &comments);
    // Add first comment manually (it's not in comments because it would
    // be at byte offset 0)
    trivia.push(Trivia::Comment {
        text: "# first".to_string(),
        byte_offset: ByteOffset::new(0),
        line: LineNum::new(1),
    });
    trivia.sort_by_key(|t| t.byte_offset());
    assert!(trivia[0].byte_offset() < trivia[1].byte_offset());
}

#[test]
fn test_annotated_atom() {
    let syntax = Syntax::new(SyntaxKind::Int(42), Span::new(0, 2, 1, 1));
    let (annotated, dangling) = AnnotatedSyntax::build_toplevel(vec![syntax], &[], "42");
    assert_eq!(annotated.len(), 1);
    assert!(matches!(annotated[0].kind(), SyntaxKind::Int(42)));
    assert!(annotated[0].leading.is_empty());
    assert!(annotated[0].children.is_empty());
    assert!(dangling.is_empty());
}

#[test]
fn test_annotated_with_leading_comment() {
    let syntax = Syntax::new(SyntaxKind::Int(42), Span::new(9, 11, 2, 1));
    let trivia = vec![Trivia::Comment {
        text: "# before".to_string(),
        byte_offset: ByteOffset::new(0),
        line: LineNum::new(1),
    }];
    let (annotated, dangling) =
        AnnotatedSyntax::build_toplevel(vec![syntax], &trivia, "# before\n42");
    assert_eq!(annotated.len(), 1);
    assert_eq!(annotated[0].leading.len(), 1);
    assert!(matches!(&annotated[0].leading[0], Trivia::Comment { text, .. } if text == "# before"));
    assert!(dangling.is_empty());
}

#[test]
fn test_annotated_list_children() {
    let arena = crate::syntax::thread_arena();
    let syntax = Syntax::list(
        &arena,
        &[
            Syntax::symbol(&arena, "+", Span::new(1, 2, 1, 2)),
            Syntax::new(SyntaxKind::Int(1), Span::new(3, 4, 1, 4)),
            Syntax::new(SyntaxKind::Int(2), Span::new(5, 6, 1, 6)),
        ],
        Span::new(0, 7, 1, 1),
    );
    let (annotated, _dangling) = AnnotatedSyntax::build_toplevel(vec![syntax], &[], "(+ 1 2)");
    assert_eq!(annotated[0].children.len(), 3);
}

/// `(a` then `tail` then `)`: a list whose one child is the symbol `a`.
fn list_of_a(tail: &str) -> (String, Syntax) {
    let arena = crate::syntax::thread_arena();
    let source = format!("(a{tail})");
    let close = source.len();
    let syntax = Syntax::list(
        &arena,
        &[Syntax::symbol(&arena, "a", Span::new(1, 2, 1, 2))],
        Span::new(0, close, 1, 1),
    );
    (source, syntax)
}

#[test]
fn the_last_form_keeps_a_comment_on_its_line_as_trailing() {
    let syntax = Syntax::new(SyntaxKind::Int(42), Span::new(0, 2, 1, 1));
    let comments = [CommentInfo {
        text: "# c".to_string(),
        offset: ByteOffset::new(4),
        line: LineNum::new(1),
    }];
    let source = "42  # c";
    let trivia = collect_trivia(source, &comments);
    let (annotated, dangling) = AnnotatedSyntax::build_toplevel(vec![syntax], &trivia, source);
    assert_eq!(annotated[0].trailing.len(), 1);
    assert!(dangling.is_empty());
}

#[test]
fn an_own_line_comment_after_the_last_child_is_closing_trivia() {
    let (source, syntax) = list_of_a("\n# c\n");
    let comments = [CommentInfo {
        text: "# c".to_string(),
        offset: ByteOffset::new(3),
        line: LineNum::new(2),
    }];
    let trivia = collect_trivia(&source, &comments);
    let (annotated, _) = AnnotatedSyntax::build_toplevel(vec![syntax], &trivia, &source);
    let a = &annotated[0].children[0];
    assert!(a.trailing.is_empty(), "{:?}", a.trailing);
    assert_eq!(a.closing.len(), 1);
}

#[test]
fn blank_lines_no_comment_follows_are_not_closing_trivia() {
    let (source, syntax) = list_of_a("\n\n");
    let trivia = collect_trivia(&source, &[]);
    assert_eq!(trivia.len(), 1, "the source holds one blank line");
    let (annotated, _) = AnnotatedSyntax::build_toplevel(vec![syntax], &trivia, &source);
    assert!(annotated[0].children[0].closing.is_empty());
}
