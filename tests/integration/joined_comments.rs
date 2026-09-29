// audited: 2026-09-28
// Fails on an Elle comment that continues a trailing comment on the line above.
// docs/fmt.md
// tests/AGENTS.md

use elle::epoch::prescan_epoch;
use elle::epoch::rules::Lexicon;
use elle::formatter::comments::{lex_for_format, strip_shebang};
use std::path::{Path, PathBuf};

/// One comment the reader's lexer found, and whether code precedes it on its
/// line.
///
/// The lexer is the only judge of what a comment is. A scan over bytes has to
/// re-derive string escapes and symbol boundaries, and gets them wrong: `a#b`
/// is one symbol, and a trailing comment needs one space before it, not the
/// two `elle fmt` writes.
struct Comment {
    line: usize,
    trailing: bool,
}

fn comments(path: &str, source: &str) -> Vec<Comment> {
    let epoch = prescan_epoch(source).unwrap_or_else(|e| panic!("{path}: {e}"));
    let (body, shebang) = strip_shebang(source);
    let lexed = lex_for_format(body, path, Lexicon::for_epoch(epoch))
        .unwrap_or_else(|e| panic!("{path}: {e}"));
    let shebang_lines = shebang.lines().count();
    lexed
        .comment_map
        .comments()
        .iter()
        .map(|c| {
            let offset = c.byte_offset.get();
            let line_start = body[..offset].rfind('\n').map_or(0, |i| i + 1);
            Comment {
                line: c.line.get() as usize + shebang_lines,
                trailing: !body[line_start..offset].trim().is_empty(),
            }
        })
        .collect()
}

/// Every trailing comment whose next line is a comment of its own. The line
/// below reads as a continuation, whatever its author meant, so a block
/// comment that starts a new subject sits behind a blank line (docs/fmt.md).
fn joined_comments(path: &str, source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    comments(path, source)
        .windows(2)
        .filter(|pair| pair[0].trailing && !pair[1].trailing && pair[1].line == pair[0].line + 1)
        .map(|pair| {
            let (above, below) = (pair[0].line, pair[1].line);
            format!("{path}:{above}: {}\n{}", lines[above - 1], lines[below - 1])
        })
        .collect()
}

fn scan(dir: &Path, root: &Path, offenders: &mut Vec<String>) {
    for entry in std::fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display())) {
        let entry = entry.unwrap_or_else(|e| panic!("read entry in {}: {e}", dir.display()));
        let path = entry.path();
        let name = entry.file_name();
        if path.is_dir() {
            if name != ".git" && name != "target" {
                scan(&path, root, offenders);
            }
        } else if path.extension().is_some_and(|ext| ext == "lisp") {
            let source = std::fs::read_to_string(&path)
                .unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
            let relative = path
                .strip_prefix(root)
                .expect("source is under repository root");
            let relative = relative.to_str().expect("repository path is UTF-8");
            offenders.extend(joined_comments(relative, &source));
        }
    }
}

#[test]
fn the_check_finds_a_comment_continued_after_code() {
    let found = joined_comments(
        "fixture.lisp",
        "(form)  # describes the next form\n# in more detail\n\
         (form)  ## another form\n## another continuation\n",
    );
    assert_eq!(found.len(), 2, "both comment prefixes must be detected");
}

#[test]
fn the_check_ignores_comment_markers_inside_multiline_strings() {
    let source = "(form \"first\n  # text\nlast\")\n# standalone\n(next)\n";
    assert!(joined_comments("fixture.lisp", source).is_empty());
}

#[test]
fn the_check_finds_a_continuation_after_a_one_space_trailing_comment() {
    // The counter-factual: a scanner that recognizes a trailing comment only
    // by the two spaces `elle fmt` puts before it passes a hand-edited file
    // that joins the same two comments with one space.
    let found = joined_comments("fixture.lisp", "(form) # describes the next form\n# in more detail\n");
    assert_eq!(found.len(), 1, "a single space still makes a trailing comment");
}

#[test]
fn the_check_finds_a_continuation_after_a_multiline_string_closes() {
    let source = "(form \"first\nlast\")  # describes the next form\n# in more detail\n";
    assert_eq!(joined_comments("fixture.lisp", source).len(), 1);
}

#[test]
fn a_blank_line_separates_a_trailing_comment_from_the_next_block() {
    let source = "(form)  # about this form\n\n# about the next form\n(next)\n";
    assert!(joined_comments("fixture.lisp", source).is_empty());
}

#[test]
fn a_hash_inside_a_symbol_starts_no_comment() {
    let source = "(def a#b 1)\n# about the next form\n(next)\n";
    assert!(joined_comments("fixture.lisp", source).is_empty());
}

#[test]
fn source_files_have_no_joined_comment_continuations() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let mut offenders = Vec::new();
    for dir in [
        "src",
        "lib",
        "tests",
        "tools",
        "demos",
        "benches",
        "examples",
        "elle-plugin",
    ] {
        let path = root.join(dir);
        if path.exists() {
            scan(&path, &root, &mut offenders);
        }
    }
    assert!(
        offenders.is_empty(),
        "joined comment continuations found:\n{}",
        offenders.join("\n")
    );
}
