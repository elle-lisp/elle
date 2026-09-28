// audited: 2026-09-28
// Reject Elle comments continued after inline comments.
// docs/fmt.md
// tests/AGENTS.md

use std::path::{Path, PathBuf};

fn trailing_comment<'a>(
    line: &'a str,
    in_string: &mut bool,
    escaped: &mut bool,
) -> Option<&'a str> {
    for (i, byte) in line.bytes().enumerate() {
        if *in_string {
            if *escaped {
                *escaped = false;
            } else if byte == b'\\' {
                *escaped = true;
            } else if byte == b'"' {
                *in_string = false;
            }
        } else if byte == b'"' {
            *in_string = true;
        } else if byte == b'#' {
            if line[..i].ends_with("  ") && !line[..i].trim().is_empty() {
                return Some(&line[i..]);
            }
            return None;
        }
    }
    None
}

fn allowed_continuation(path: &str, inline: &str, next: &str) -> bool {
    let cases = [
        ("lib/compress.lisp", "# avail_out = 32", "## Compress"),
        ("lib/tls.lisp", "# Plaintext buffer empty", "# Use 16384"),
        (
            "demos/test-h2-stress.lisp",
            "# Possibly read server WINDOW_UPDATE for conn",
            "# Send our SETTINGS ACK",
        ),
        (
            "tests/elle/sync.lisp",
            "# let waiter B reach its park",
            "# Wake ONLY fxB",
        ),
        (
            "tests/elle/lib/estimator.lisp",
            "# warmup block, discarded",
            "# Resolved once, outside every measurement window",
        ),
        (
            "tests/elle/lib/estimator.lisp",
            "# warmup block, discarded",
            "# Both readings resolved once",
        ),
    ];
    cases.iter().any(|(file, start, continuation)| {
        path == *file && inline.starts_with(start) && next.starts_with(continuation)
    })
}

fn joined_comments(path: &str, source: &str) -> Vec<String> {
    let lines: Vec<&str> = source.lines().collect();
    let mut in_string = false;
    let mut escaped = false;
    let comments: Vec<Option<&str>> = lines
        .iter()
        .map(|line| trailing_comment(line, &mut in_string, &mut escaped))
        .collect();
    lines
        .windows(2)
        .enumerate()
        .filter_map(|(i, pair)| {
            let inline = comments[i]?;
            let continuation = pair[1].trim_start();
            if continuation.starts_with('#') && !allowed_continuation(path, inline, continuation) {
                Some(format!("{path}:{}: {}\n{}", i + 1, pair[0], pair[1]))
            } else {
                None
            }
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
