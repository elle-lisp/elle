//! Unit tests (`super` is the parent impl module).

use super::*;

#[test]
fn test_empty_source() {
    let map = CommentMap::collect("", "<test>").unwrap();
    assert!(map.is_empty());
}

#[test]
fn test_no_comments() {
    let map = CommentMap::collect("(+ 1 2)", "<test>").unwrap();
    assert!(map.is_empty());
}

#[test]
fn test_single_comment() {
    let map = CommentMap::collect("# hello", "<test>").unwrap();
    assert_eq!(map.comments().len(), 1);
    assert_eq!(map.comments()[0].text, "# hello");
    assert_eq!(map.comments()[0].line, LineNum::new(1));
}

#[test]
fn test_multiple_comments() {
    let map = CommentMap::collect("# first\n# second\n(+ 1 2)", "<test>").unwrap();
    assert_eq!(map.comments().len(), 2);
    assert_eq!(map.comments()[0].text, "# first");
    assert_eq!(map.comments()[1].text, "# second");
}

#[test]
fn test_doc_comment() {
    let map = CommentMap::collect("## doc text", "<test>").unwrap();
    assert_eq!(map.comments().len(), 1);
    assert!(map.comments()[0].text.starts_with("##"));
}

#[test]
fn test_take_leading() {
    let mut map = CommentMap::collect("# before\n42 # inline\n# after", "<test>").unwrap();
    assert_eq!(map.comments().len(), 3);

    let leading = map.take_leading(ByteOffset::new(10)); // byte offset of "42"
    assert_eq!(leading.len(), 1);
    assert_eq!(leading[0].text, "# before");
    assert_eq!(map.comments().len(), 2);
}

#[test]
fn test_take_trailing() {
    let mut map = CommentMap::collect("42 # inline\n# after", "<test>").unwrap();
    let trailing = map.take_trailing(LineNum::new(1));
    assert_eq!(trailing.len(), 1);
    assert_eq!(trailing[0].text, "# inline");
    assert_eq!(map.comments().len(), 1);
}

#[test]
fn test_lex_for_format() {
    let result = lex_for_format("# comment\n(+ 1 2)", "<test>").unwrap();
    // Regular tokens: (, +, 1, 2, )
    assert_eq!(result.tokens.len(), 5);
    // Comment map has 1 comment
    assert_eq!(result.comment_map.comments().len(), 1);
}

#[test]
fn test_strip_shebang() {
    let (source, shebang) = strip_shebang("#!/usr/bin/env elle\n(+ 1 2)");
    assert_eq!(shebang, "#!/usr/bin/env elle\n");
    assert_eq!(source, "(+ 1 2)");
}

#[test]
fn test_strip_no_shebang() {
    let (source, shebang) = strip_shebang("(+ 1 2)");
    assert_eq!(shebang, "");
    assert_eq!(source, "(+ 1 2)");
}

#[test]
fn test_lex_for_format_shebang() {
    // lex_for_format receives already-stripped source
    let (stripped, _shebang) = strip_shebang("#!/usr/bin/env elle\n(+ 1 2)");
    let result = lex_for_format(stripped, "<test>").unwrap();
    assert_eq!(result.tokens.len(), 5);
    assert!(result.comment_map.is_empty());
}
