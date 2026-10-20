// audited: 2026-10-20
// The size producer reads each source over the cap at its bucket, the excess over all of them, and nothing else.
// tests/ratchet/overview.md
//
// The counter-factuals a ledger of real seeds cannot give: every row is green
// the day it is seeded, so a producer that read nothing, or read a document,
// or read a line count, would look exactly like one that works. Each test
// runs the producer from a scratch tree whose sizes it chose.
//
// Each fixture line below costs exactly ten tokens of Rust or eight of Lisp in
// the `o200k_base` encoding, measured apart from the rig.

mod common;

use common::{audited_tree, repo, rig, stderr, stdout, Scratch};
use std::collections::BTreeMap;
use std::process::{Command, Output};

/// A Rust line of ten tokens.
const RUST: &str = "let walked = reach.charge(edge, pace);\n";

/// A Lisp line of eight tokens.
const LISP: &str = "(def walked (charge reach edge pace))\n";

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
fn a_source_at_the_cap_a_document_and_an_exempt_file_read_nothing() {
    // The counter-factual: a producer that walked every file, or read a source
    // at the cap as over it, leaves rows no remedy is owed.
    let t = audited_tree("size-none");
    t.write("src/at.rs", &RUST.repeat(250));
    t.write("src/small.rs", &RUST.repeat(20));
    t.write(
        "docs/long.md",
        &"A sentence a reader pays for.\n".repeat(400),
    );
    t.write("vendor/LICENSE-MIT", "MIT\n");
    t.write("vendor/tables.rs", &RUST.repeat(400));
    t.write("src/stdlib.lisp", &LISP.repeat(400));
    if gated(&t) {
        return;
    }
    let read = readings(&produce(&t));
    let paths: Vec<&str> = read
        .keys()
        .map(String::as_str)
        .filter(|s| !s.starts_with("the excess"))
        .collect();
    assert!(paths.is_empty(), "no file here reads: {read:?}");
}

#[test]
fn the_excess_is_each_reading_less_the_cap_summed() {
    // The counter-factual: a total over exact token counts moves on every
    // edit, and a total that skipped a source lets a new one in unseen.
    let t = audited_tree("size-excess");
    t.write("src/dense.rs", &RUST.repeat(300));
    t.write("lib/big.lisp", &LISP.repeat(400));
    if gated(&t) {
        return;
    }
    let read = readings(&produce(&t));
    assert_eq!(
        read.get("the excess the work sheds").cloned(),
        Some(("tokens".to_string(), 1500)),
        "3,000 less the cap, and 3,500 less the cap: {read:?}"
    );
    assert_eq!(
        read.get("the excess tables carry").cloned(),
        Some(("tokens".to_string(), 0)),
        "no table is admitted, so tables carry nothing: {read:?}"
    );
    let paths: Vec<&str> = read
        .keys()
        .map(String::as_str)
        .filter(|s| !s.starts_with("the excess"))
        .collect();
    assert_eq!(
        paths,
        ["lib/big.lisp", "src/dense.rs"],
        "the two fixtures and nothing else: {read:?}"
    );
}
