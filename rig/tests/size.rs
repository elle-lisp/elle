// audited: 2026-10-20
// The size producer reads each source and document over its cap at its bucket, the excess over all of them, and nothing else.
// tests/ratchet/overview.md
//
// The counter-factuals a ledger of real seeds cannot give: every row is green
// the day it is seeded, so a producer that read nothing, held a document to
// the source cap, or read a line count, would look exactly like one that
// works. Each test runs the producer from a scratch tree whose sizes it chose.
//
// Each fixture line below costs exactly ten tokens of Rust or eight of Lisp in
// the `o200k_base` encoding, measured apart from the rig. A line of prose
// costs seven, and a document's title three, measured with `elle/tokens`,
// whose encoding tokens.rs pins; each count holds over repeated lines.

mod common;

use common::{audited_tree, repo, rig, stderr, stdout, Scratch};
use std::collections::BTreeMap;
use std::process::{Command, Output};

/// A Rust line of ten tokens.
const RUST: &str = "let walked = reach.charge(edge, pace);\n";

/// A Lisp line of eight tokens.
const LISP: &str = "(def walked (charge reach edge pace))\n";

/// A line of prose of seven tokens.
const PROSE: &str = "A sentence a reader pays for.\n";

/// A document of `lines` lines of prose under a title of three tokens.
fn document(title: &str, lines: usize) -> String {
    format!("# {title}\n\n{}", PROSE.repeat(lines))
}

/// The build whose rows the size ledger holds (docs/ratchet.md).
const REFERENCE: &str = "jit-uring-linux-x86_64";

/// The key of the build this rig is.
fn key() -> String {
    stdout(&rig(&["-e", "(print (elle/build))"]))
        .trim()
        .to_string()
}

/// The producer, run directly from the scratch tree.
fn produce(t: &Scratch) -> Output {
    Command::new(env!("CARGO_BIN_EXE_elle-rig"))
        .current_dir(t.path())
        .arg(repo().join("tests/ratchet/size.lisp"))
        .output()
        .expect("spawn elle-rig")
}

/// The text of one `"field":` in a reading line: a string's contents, or a
/// number as written.
fn field<'a>(line: &'a str, name: &str) -> &'a str {
    let at = line
        .find(&format!("\"{name}\":"))
        .unwrap_or_else(|| panic!("no {name} in {line}"));
    let rest = &line[at + name.len() + 3..];
    match rest.strip_prefix('"') {
        Some(s) => &s[..s.find('"').expect("a closed string")],
        None => &rest[..rest.find([',', '}']).expect("a number ends")],
    }
}

/// Every reading the producer printed, by subject: its axis and its value.
fn readings(out: &Output) -> BTreeMap<String, (String, u64)> {
    let text = stdout(out);
    assert!(
        out.status.success(),
        "the producer runs green:\n{text}{}",
        stderr(out)
    );
    text.lines()
        .filter_map(|l| l.strip_prefix("measure "))
        .map(|l| {
            let value: f64 = field(l, "value").parse().expect("a numeric value");
            (
                field(l, "subject").to_string(),
                (field(l, "axis").to_string(), value as u64),
            )
        })
        .collect()
}

/// Whether this rig is a build the ledger holds no rows for. On such a build
/// the producer says it skipped, and reads nothing.
fn gated(t: &Scratch) -> bool {
    if key() == REFERENCE {
        return false;
    }
    let out = produce(t);
    let both = format!("{}{}", stdout(&out), stderr(&out));
    assert!(
        out.status.success() && both.contains("SKIP (gated):"),
        "away from {REFERENCE} the producer gates itself out:\n{both}"
    );
    assert!(
        !stdout(&out).contains("measure "),
        "and reads nothing:\n{both}"
    );
    true
}

#[test]
fn a_source_over_the_cap_in_tokens_reads_at_its_bucket_whatever_its_lines() {
    // The counter-factual is the line cap: every file here is under 500 lines,
    // so the cap at 500 lines read none of them.
    let t = audited_tree("size-over");
    t.write("src/dense.rs", &RUST.repeat(300));
    t.write("src/over.rs", &RUST.repeat(251));
    t.write("lib/big.lisp", &LISP.repeat(400));
    if gated(&t) {
        return;
    }
    let read = readings(&produce(&t));
    let tokens = |path: &str| read.get(path).cloned();
    assert_eq!(
        tokens("src/dense.rs"),
        Some(("tokens".to_string(), 3000)),
        "3,000 tokens is its own bucket: {read:?}"
    );
    assert_eq!(
        tokens("src/over.rs"),
        Some(("tokens".to_string(), 3000)),
        "2,510 tokens rounds up to 3,000: {read:?}"
    );
    assert_eq!(
        tokens("lib/big.lisp"),
        Some(("tokens".to_string(), 3500)),
        "3,200 tokens of Lisp rounds up to 3,500: {read:?}"
    );
}

#[test]
fn a_document_over_its_own_cap_reads_at_its_bucket() {
    // The counter-factual is a producer that read sources alone: a document
    // would then grow past any size with no row to stop it.
    let t = audited_tree("size-document");
    t.write("docs/big.md", &document("Big", 600));
    if gated(&t) {
        return;
    }
    let read = readings(&produce(&t));
    assert_eq!(
        read.get("docs/big.md").cloned(),
        Some(("tokens".to_string(), 4500)),
        "4,203 tokens rounds up to 4,500: {read:?}"
    );
}

/// The paths the producer read, less the totals.
fn paths(read: &BTreeMap<String, (String, u64)>) -> Vec<&str> {
    read.keys()
        .map(String::as_str)
        .filter(|s| !s.starts_with("the excess"))
        .collect()
}

#[test]
fn a_file_at_its_cap_and_an_exempt_file_read_nothing() {
    // The counter-factuals: a producer that walked every file, read a file at
    // its cap as over it, or held a document to the source cap leaves rows no
    // remedy is owed. The long document is over the source cap and under its
    // own, and the index is over both and generated.
    let t = audited_tree("size-none");
    t.write("src/at.rs", &RUST.repeat(250));
    t.write("src/small.rs", &RUST.repeat(20));
    t.write("docs/long.md", &document("Big", 400));
    t.write("docs/at.md", &document("At", 571));
    t.write(
        "docs/AGENTS.md",
        &format!(
            "# docs\n\nGenerated by `scripts/agents`. Edit a document, never this index.\n\n{}",
            PROSE.repeat(700)
        ),
    );
    t.write("vendor/LICENSE-MIT", "MIT\n");
    t.write("vendor/tables.rs", &RUST.repeat(400));
    t.write("vendor/notes.md", &document("Big", 700));
    t.write("src/stdlib.lisp", &LISP.repeat(400));
    if gated(&t) {
        return;
    }
    let read = readings(&produce(&t));
    assert!(paths(&read).is_empty(), "no file here reads: {read:?}");
}

#[test]
fn the_excess_is_each_reading_less_its_cap_summed() {
    // The counter-factual: a total over exact token counts moves on every
    // edit, a total that skipped a file lets a new one in unseen, and a total
    // that took the source cap from a document charges it 2,000 too much.
    let t = audited_tree("size-excess");
    t.write("src/dense.rs", &RUST.repeat(300));
    t.write("lib/big.lisp", &LISP.repeat(400));
    t.write("docs/big.md", &document("Big", 600));
    if gated(&t) {
        return;
    }
    let read = readings(&produce(&t));
    assert_eq!(
        read.get("the excess the work sheds").cloned(),
        Some(("tokens".to_string(), 2000)),
        "3,000 and 3,500 less the source cap, and 4,500 less the document cap: {read:?}"
    );
    assert_eq!(
        read.get("the excess tables carry").cloned(),
        Some(("tokens".to_string(), 0)),
        "no table is admitted, so tables carry nothing: {read:?}"
    );
    assert_eq!(
        paths(&read),
        ["docs/big.md", "lib/big.lisp", "src/dense.rs"],
        "the three fixtures and nothing else: {read:?}"
    );
}
