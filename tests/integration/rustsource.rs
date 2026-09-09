// audited: 2026-09-09
// Reads a Rust file well enough to tell its code from its strings.
//
// docs/analysis/testing.md
//
// A test that scans the tree for a shape rustc does not check has to know
// which bytes are which. Matching a delimiter must skip the `{}` inside a
// format string, or `"…tag=0x{:x}"` closes a brace it never opened. Searching
// for a panic message must skip code and comments, or a comment that quotes a
// panic attributes it to the wrong place.
//
// This is not a Rust parser and does not want to be. It answers one question
// per byte — code, string, or comment — which is all a span-and-search scan
// needs, and it is small enough to read whole before trusting a result.

use std::fs;
use std::path::{Path, PathBuf};

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Class {
    Code,
    Str,
    Comment,
}

/// Classify every byte of a Rust file as code, string content, or comment.
///
/// The trap: `'"'` is a char literal, and 17 files under `src/` contain one. A
/// scan that read its quote as a string opener would classify the rest of that
/// file inside-out, and every span derived from it would be nonsense. So a
/// quote opens a string only where a char literal has been ruled out first.
pub fn classify(src: &str) -> Vec<Class> {
    let b = src.as_bytes();
    let n = b.len();
    let mut out = vec![Class::Code; n];
    let mark = |out: &mut Vec<Class>, from: usize, to: usize, c: Class| {
        for slot in out.iter_mut().take(to.min(n)).skip(from) {
            *slot = c;
        }
    };
    let mut i = 0;
    while i < n {
        if b[i] == b'/' && i + 1 < n && b[i + 1] == b'/' {
            let end = src[i..].find('\n').map_or(n, |k| i + k);
            mark(&mut out, i, end, Class::Comment);
            i = end;
        } else if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
            let end = block_comment_end(b, i);
            mark(&mut out, i, end, Class::Comment);
            i = end;
        } else if let Some((content, hashes)) = raw_string_open(b, i) {
            let end = raw_string_end(b, content, hashes);
            mark(&mut out, content, end, Class::Str);
            i = end + 1 + hashes;
        } else if b[i] == b'"' {
            let end = string_end(b, i + 1);
            mark(&mut out, i + 1, end, Class::Str);
            i = end + 1;
        } else if b[i] == b'\'' {
            // A lifetime and a char literal open the same way. Only the
            // literal consumes a closing quote, so only it can hide a `"`.
            i = match char_literal_end(src, i) {
                Some(end) => end + 1,
                None => i + 1,
            };
        } else {
            i += 1;
        }
    }
    out
}

/// The index past a block comment, counting the nesting Rust allows.
fn block_comment_end(b: &[u8], open: usize) -> usize {
    let (n, mut depth, mut i) = (b.len(), 1usize, open + 2);
    while i < n && depth > 0 {
        if b[i] == b'/' && i + 1 < n && b[i + 1] == b'*' {
            depth += 1;
            i += 2;
        } else if b[i] == b'*' && i + 1 < n && b[i + 1] == b'/' {
            depth -= 1;
            i += 2;
        } else {
            i += 1;
        }
    }
    i
}

/// The content start and hash count of a raw string opening at `i`.
///
/// The identifier check is what stops `for_r "x"` — or any name ending in `r`
/// — from opening one.
fn raw_string_open(b: &[u8], i: usize) -> Option<(usize, usize)> {
    let ident = |c: u8| c.is_ascii_alphanumeric() || c == b'_';
    if i > 0 && ident(b[i - 1]) {
        return None;
    }
    let mut j = i;
    if b.get(j) == Some(&b'b') {
        j += 1;
    }
    if b.get(j) != Some(&b'r') {
        return None;
    }
    j += 1;
    let first_hash = j;
    while b.get(j) == Some(&b'#') {
        j += 1;
    }
    if b.get(j) != Some(&b'"') {
        return None;
    }
    Some((j + 1, j - first_hash))
}

/// A raw string ends at the quote carrying its own hash count back. It honors
/// no escape, so a trailing backslash does not extend it.
fn raw_string_end(b: &[u8], content: usize, hashes: usize) -> usize {
    let n = b.len();
    let mut i = content;
    while i < n {
        let closes = b[i] == b'"' && b[i + 1..].iter().take(hashes).all(|c| *c == b'#');
        if closes {
            return i;
        }
        i += 1;
    }
    n
}

fn string_end(b: &[u8], content: usize) -> usize {
    let n = b.len();
    let mut i = content;
    while i < n {
        match b[i] {
            b'\\' => i += 2,
            b'"' => return i,
            _ => i += 1,
        }
    }
    n
}

/// The index of a char literal's closing quote, when `i` opens one rather than
/// a lifetime.
fn char_literal_end(src: &str, i: usize) -> Option<usize> {
    let rest = &src[i + 1..];
    let mut chars = rest.char_indices();
    let (_, first) = chars.next()?;
    if first == '\\' {
        // `'\n'`, `'\''`, `'\u{1f600}'` — the closing quote is the first one
        // past the escape. Past the WHOLE escape: `\'` escapes a quote, so a
        // search that skipped only the backslash would stop on the character
        // being escaped and call the real closing quote an opening one.
        let (_, escaped) = chars.next()?;
        let past = 1 + escaped.len_utf8();
        return rest[past..].find('\'').map(|k| i + 1 + past + k);
    }
    let (second_at, second) = chars.next()?;
    (second == '\'').then_some(i + 1 + second_at)
}

/// One source file, read three ways.
///
/// `code` blanks the strings and the comments, so a delimiter found in it is
/// really a delimiter. `text` blanks only the comments, so a span taken from
/// `code` can be read back as source. `cls` says which bytes of that span are
/// string content. All three keep the byte offsets the original had, so one
/// span means the same region in every one of them.
pub struct Source {
    pub path: PathBuf,
    pub cls: Vec<Class>,
    pub code: String,
    pub text: String,
}

impl Source {
    pub fn read(path: PathBuf) -> Self {
        let src =
            fs::read_to_string(&path).unwrap_or_else(|e| panic!("read {}: {e}", path.display()));
        Source::of(path, &src)
    }

    pub fn of(path: PathBuf, src: &str) -> Self {
        let cls = classify(src);
        let mut code = String::with_capacity(src.len());
        let mut text = String::with_capacity(src.len());
        for (i, ch) in src.char_indices() {
            let blanks = " ".repeat(ch.len_utf8());
            match cls[i] {
                Class::Comment => {
                    code.push_str(&blanks);
                    text.push_str(&blanks);
                }
                Class::Str => {
                    code.push_str(&blanks);
                    text.push(ch);
                }
                Class::Code => {
                    code.push(ch);
                    text.push(ch);
                }
            }
        }
        Source { path, cls, code, text }
    }

    /// Every string literal inside one span, as it was written.
    pub fn literals(&self, (from, to): (usize, usize)) -> Vec<&str> {
        let end = to.min(self.cls.len());
        let mut out = Vec::new();
        let mut i = from;
        while i < end {
            if self.cls[i] != Class::Str {
                i += 1;
                continue;
            }
            let start = i;
            while i < end && self.cls[i] == Class::Str {
                i += 1;
            }
            out.push(&self.text[start..i]);
        }
        out
    }
}

/// The index closing the delimiter that opens at `open`.
pub fn matching(code: &str, open: usize) -> Option<usize> {
    let mut depth = 0usize;
    for (i, c) in code.as_bytes().iter().enumerate().skip(open) {
        match c {
            b'(' | b'[' | b'{' => depth += 1,
            b')' | b']' | b'}' => {
                depth = depth.checked_sub(1)?;
                if depth == 0 {
                    return Some(i);
                }
            }
            _ => {}
        }
    }
    None
}

/// Every start offset of `needle`, including the overlapping ones.
pub fn find_all(hay: &str, needle: &str) -> Vec<usize> {
    let mut out = Vec::new();
    let mut from = 0;
    while let Some(k) = hay[from..].find(needle) {
        out.push(from + k);
        from += k + 1;
    }
    out
}

/// Every `.rs` file under one directory, deepest last, in a stable order.
pub fn rust_sources(dir: &Path) -> Vec<PathBuf> {
    let mut out = Vec::new();
    let entries = fs::read_dir(dir).unwrap_or_else(|e| panic!("read {}: {e}", dir.display()));
    for entry in entries {
        let path = entry.expect("read a directory entry").path();
        if path.is_dir() {
            out.extend(rust_sources(&path));
        } else if path.extension().is_some_and(|e| e == "rs") {
            out.push(path);
        }
    }
    out.sort();
    out
}

// ── Tests ───────────────────────────────────────────────────────────

#[cfg(test)]
fn classes(src: &str) -> String {
    classify(src)
        .iter()
        .map(|c| match c {
            Class::Code => 'c',
            Class::Str => 's',
            Class::Comment => '#',
        })
        .collect()
}

#[test]
fn a_string_is_string_and_the_code_around_it_is_code() {
    // A quote is a delimiter, so it stays code; only what it encloses is text.
    assert_eq!(classes(r#"f("ab");"#), "cccssccc");
}

#[test]
fn an_escaped_quote_does_not_end_its_string() {
    // Counter-factual: read `\"` as a terminator and the rest of the file
    // classifies inside-out — code as string, string as code.
    assert_eq!(classes(r#""a\"b" x"#), "cssssccc");
}

#[test]
fn a_quote_char_literal_does_not_open_a_string() {
    // The trap this file exists for. `'"'` appears in 17 files under `src/`,
    // and taking its quote as an opener inverts everything after it.
    assert_eq!(classes("'\"' + 1;"), "cccccccc");
}

#[test]
fn a_lifetime_is_not_a_char_literal() {
    assert_eq!(classes("&'a str"), "ccccccc");
}

#[test]
fn an_escaped_char_literal_ends_past_its_whole_escape() {
    // `'\''` holds two quotes, and only the second closes it. Counter-factual:
    // stop at the first and the literal ends one byte early, so the closing
    // quote opens a fresh scan — which, on `'\u{27}'`, swallows the byte after
    // the literal instead.
    assert_eq!(char_literal_end(r"'\''", 0), Some(3));
    assert_eq!(char_literal_end(r"'\u{27}'", 0), Some(7));
    assert_eq!(char_literal_end(r"'\n'", 0), Some(3));
    assert_eq!(classes(r#"'\'' "x""#), "ccccccsc");
}

#[test]
fn a_raw_string_honors_no_escape() {
    // `\` is content in a raw string, so it closes at the first quote that
    // carries its hash count back — here the one right after the backslash.
    assert_eq!(classes(r###"r#"a\"#;"###), "cccssccc");
}

#[test]
fn an_identifier_ending_in_r_does_not_open_a_raw_string() {
    assert_eq!(classes(r#"or"x""#), "cccsc");
}

#[test]
fn a_comment_runs_to_its_end_and_a_nested_block_closes_once() {
    // The newline ends a line comment and is not part of it.
    assert_eq!(classes("a // b\nc"), "cc####cc");
    assert_eq!(classes("/*a/*b*/c*/d"), "###########c");
}

#[test]
fn a_brace_inside_a_string_does_not_match_a_delimiter() {
    // Counter-factual: match on the raw text and `{:x}` closes the argument
    // list early, so every span after it is short and the scan sees nothing.
    let src = Source::of(PathBuf::from("x.rs"), r#"panic!("v={:x}", n); after"#);
    let open = src.code.find('(').expect("an argument list");
    let close = matching(&src.code, open).expect("its close");
    assert_eq!(&src.code[open..=close], "(\"      \", n)");
}

#[test]
fn literals_reads_the_strings_of_a_span_and_nothing_else() {
    let src = Source::of(PathBuf::from("x.rs"), r#"f("a{b}", c, "d") // "e""#);
    let open = src.code.find('(').expect("an argument list");
    let close = matching(&src.code, open).expect("its close");
    assert_eq!(src.literals((open, close)), vec!["a{b}", "d"]);
}
