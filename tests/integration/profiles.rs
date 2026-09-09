// audited: 2026-09-09
// A `#[should_panic]` test in `src/` runs in the profile whose panic it expects.
//
// docs/analysis/testing.md
//
// `debug_assert!` and `#[cfg(debug_assertions)]` compile to nothing in a
// release build. A test that expects one of those panics therefore passes
// under `cargo test` and fails under `cargo test --release -p elle --lib`,
// where no panic arrives and libtest reports "test did not panic as expected".
//
// No CI job builds a release test harness, so nothing reports that. The
// profile is read only when somebody runs it, which is what reading a counter
// kept beside a compiled-out assert requires — and a real failure there is
// invisible in a suite already red for this reason.
//
// So this scan stands in for the job that does not exist. It reads the panic
// sites out of `src/` and fails when an ungated `#[should_panic]` names a
// message only a debug build can produce.

use super::rustsource::{find_all, matching, rust_sources, Source};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

/// A panic message head shorter than this is not attributed. Two unrelated
/// sites share a short phrase often enough that the scan would report the
/// wrong one, and a message that short does not pin its own test either.
const MIN_MESSAGE: usize = 8;

/// The macros and calls that raise a panic, and whose argument list therefore
/// holds a message a `#[should_panic]` test can name.
const PANIC_CALLS: &[&str] = &[
    "debug_assert!(",
    "debug_assert_eq!(",
    "debug_assert_ne!(",
    "panic!(",
    "assert!(",
    "assert_eq!(",
    "assert_ne!(",
    "unreachable!(",
    "todo!(",
    "unimplemented!(",
    ".expect(",
];

/// The part of a panic message that is the same at the site and in the test.
///
/// A site writes `"DecrefRegion({id}) but …"` and its test expects
/// `"DecrefRegion(4) but …"`; a site writes `"stale region deref: {ptr:?} …"`
/// and its test expects `"stale region"`. Neither string contains the other,
/// and neither is reliably the shorter. What both agree on is the run before
/// the first substitution, so that run is what the two are compared by.
fn head(message: &str) -> &str {
    message.split(['{', '}', '\\', '\n']).next().unwrap_or("").trim()
}

/// Every region a debug build compiles and a release build drops: the whole of
/// a `debug_assert!` call, the item a `#[cfg(debug_assertions)]` attribute
/// gates, and the block an `if cfg!(debug_assertions)` guards.
fn debug_only_spans(code: &str) -> Vec<(usize, usize)> {
    let mut spans = Vec::new();

    for start in find_all(code, "debug_assert") {
        let tail = &code[start..];
        let open = ["debug_assert!(", "debug_assert_eq!(", "debug_assert_ne!("]
            .iter()
            .find(|form| tail.starts_with(**form))
            .map(|form| start + form.len() - 1);
        if let Some(close) = open.and_then(|o| matching(code, o)) {
            spans.push((start, close));
        }
    }

    for at in find_all(code, "debug_assertions") {
        let back = &code[at.saturating_sub(64)..at];
        let opener = back.rfind("#[").filter(|k| !back[*k..].contains(']'));
        let after_opener = opener.map_or(back, |k| &back[k..]);
        // `#[cfg(not(debug_assertions))]` gates the opposite profile, and a
        // release-only panic is not what this scan looks for.
        if after_opener.contains("not(") {
            continue;
        }
        let item = if opener.is_some() {
            // The attribute closes, then the item it gates begins. That item
            // is a block, or a `use`/`let` ending at its semicolon.
            code[at..].find(']').map(|k| at + k + 1)
        } else if after_opener.contains("cfg!(") {
            Some(at)
        } else {
            continue;
        };
        let Some(item) = item else { continue };
        let brace = code[item..].find('{').map(|k| item + k);
        let semi = code[item..].find(';').map(|k| item + k);
        match (brace, semi) {
            (Some(brace), semi) if semi.is_none_or(|s| brace < s) => {
                if let Some(close) = matching(code, brace) {
                    spans.push((item, close));
                }
            }
            (_, Some(semi)) => spans.push((item, semi)),
            _ => {}
        }
    }
    spans
}

/// The message head of every panic a release build cannot raise, taken from
/// the argument list of the call that raises it.
///
/// The narrowing to a panic's own arguments is what keeps the scan quiet. A
/// debug-only function may name `"%string-push"` in a lookup table without
/// ever panicking with it, and a whole-region search would read that as the
/// panic site of the test that expects `%string-push`.
fn debug_only_heads(src: &Source) -> Vec<String> {
    let mut out = Vec::new();
    for (from, to) in debug_only_spans(&src.code) {
        for call in PANIC_CALLS {
            for at in find_all(&src.code[from..to], call) {
                let open = from + at + call.len() - 1;
                let Some(close) = matching(&src.code, open) else {
                    continue;
                };
                for literal in src.literals((open, close)) {
                    let h = head(literal);
                    if h.len() >= MIN_MESSAGE {
                        out.push(h.to_string());
                    }
                }
            }
        }
    }
    out
}

struct ShouldPanic {
    file: PathBuf,
    line: usize,
    expected: Option<String>,
    gated: bool,
}

/// Every `#[should_panic]` test in one file, with the message it names and
/// whether its own attributes gate it to a debug build.
///
/// A gate sits before or after `#[should_panic]` — both orders are in the tree
/// — so the whole attribute run around it is read. Only `#[…]` lines count
/// toward the gate: a doc comment above such a test often discusses
/// `debug_assertions` in prose, and reading that as a gate would silence the
/// scan on exactly the tests it exists for.
fn should_panic_tests(text: &str, file: &Path) -> Vec<ShouldPanic> {
    let lines: Vec<&str> = text.lines().collect();
    let attr = |i: usize| lines[i].trim_start().starts_with("#[");
    let skippable = |i: usize| attr(i) || lines[i].trim_start().starts_with("//");

    let mut out = Vec::new();
    for (i, line) in lines.iter().enumerate() {
        if !line.trim_start().starts_with("#[should_panic") {
            continue;
        }
        let mut block = vec![i];
        let mut up = i;
        while up > 0 && skippable(up - 1) {
            up -= 1;
            if attr(up) {
                block.push(up);
            }
        }
        let mut down = i;
        while down + 1 < lines.len() && skippable(down + 1) {
            down += 1;
            if attr(down) {
                block.push(down);
            }
        }
        out.push(ShouldPanic {
            file: file.to_path_buf(),
            line: i + 1,
            expected: expected_message(line),
            gated: block.iter().any(|k| lines[*k].contains("debug_assertions")),
        });
    }
    out
}

/// The `expected = "…"` message of a `#[should_panic]` attribute.
fn expected_message(line: &str) -> Option<String> {
    let at = line.find("expected")?;
    let open = line[at..].find('"')? + at + 1;
    let close = line[open..].find('"')? + open;
    Some(line[open..close].to_string())
}

#[test]
fn a_should_panic_test_names_the_profile_whose_panic_it_expects() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    let sources: Vec<Source> = rust_sources(&root.join("src"))
        .into_iter()
        .map(Source::read)
        .collect();
    assert!(!sources.is_empty(), "the scan read no source files");

    // Crate-wide: a test names the panic of whatever file raises it, and the
    // region checks are asserted several modules from the tests that drive
    // them.
    let heads: Vec<String> = sources.iter().flat_map(debug_only_heads).collect();
    assert!(!heads.is_empty(), "the scan found no debug-only panic sites");

    let mut problems = String::new();
    for src in &sources {
        let shown = src.path.strip_prefix(&root).unwrap_or(&src.path);
        for test in should_panic_tests(&src.text, shown) {
            let Some(expected) = test.expected else {
                writeln!(
                    problems,
                    "{}:{}: #[should_panic] names no expected message, so nothing \
                     can say which panic it waits for",
                    test.file.display(),
                    test.line,
                )
                .expect("write to a String");
                continue;
            };
            if test.gated {
                continue;
            }
            let want = head(&expected);
            if want.len() < MIN_MESSAGE {
                continue;
            }
            // A message can be named by more than one debug-only site — the
            // check itself, and a test of the check that quotes it. Report the
            // one the test waits for, which is the site whose own message
            // opens with what the test named.
            let site = heads
                .iter()
                .find(|h| h.starts_with(want) || want.starts_with(h.as_str()))
                .or_else(|| heads.iter().find(|h| h.contains(want) || want.contains(*h)));
            if let Some(site) = site {
                writeln!(
                    problems,
                    "{}:{}: expects {expected:?}, and only a debug build panics \
                     with {site:?} — add #[cfg(debug_assertions)]",
                    test.file.display(),
                    test.line,
                )
                .expect("write to a String");
            }
        }
    }

    assert!(
        problems.is_empty(),
        "a #[should_panic] test whose panic comes from a debug_assert! passes \
         under `cargo test` and fails under `cargo test --release -p elle \
         --lib` (docs/analysis/testing.md):\n{problems}"
    );
}
