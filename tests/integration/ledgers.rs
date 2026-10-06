// audited: 2026-10-05
// The committed ledgers: every producer on the ratchet has one, every row of
// one names a subject its producer reads, and no row is in two files.
//
// docs/ratchet.md
//
// The counter-factual: a row outlives the probe that read it, and `missing`
// only says so once the producer runs, which for a dashboard is a minute into
// the implementation suite. These read the ledgers as text and fail in a
// second.

use std::path::{Path, PathBuf};

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// One committed ledger file: the producer it answers for, and the subject,
/// the axis and the build of every row in it.
struct Ledger {
    file: PathBuf,
    producer: String,
    subjects: Vec<String>,
    keys: Vec<String>,
}

/// The producer the runner reads each file's charge for; its subjects are
/// files, not string literals in a source (docs/test-gauges.md).
const RUNNER: &str = "elle test";

/// The string literal opening at `text`'s first byte, unescaped.
fn string_literal(text: &str) -> Option<String> {
    let mut chars = text.strip_prefix('"')?.chars();
    let mut out = String::new();
    loop {
        match chars.next()? {
            '\\' => out.push(chars.next()?),
            '"' => return Some(out),
            c => out.push(c),
        }
    }
}

/// Every ledger under `tests/ledger`, read the way the row reader reads it: a
/// `(producer "…")` header, then one `["subject" :axis …]` row per line.
fn ledgers() -> Vec<Ledger> {
    let dir = repo_root().join("tests/ledger");
    let mut out = Vec::new();
    for entry in std::fs::read_dir(&dir).expect("tests/ledger exists") {
        let file = entry.expect("a directory entry").path();
        if file.extension().and_then(|e| e.to_str()) != Some("lisp") {
            continue;
        }
        let text = std::fs::read_to_string(&file).expect("read the ledger");
        let mut producer = None;
        let mut subjects = Vec::new();
        let mut keys = Vec::new();
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("(producer ") {
                producer = string_literal(rest);
            } else if let Some(rest) = line.strip_prefix('[') {
                if let Some(subject) = string_literal(rest) {
                    keys.push(row_key(&subject, rest));
                    subjects.push(subject);
                }
            }
        }
        let producer = producer.unwrap_or_else(|| panic!("{} names no producer", file.display()));
        out.push(Ledger {
            file,
            producer,
            subjects,
            keys,
        });
    }
    out
}

/// What makes a row one row: its subject, the axis after it, and the build a
/// `:build` names, or none. `rest` is the row's line after its bracket, which
/// opens with the subject's literal.
fn row_key(subject: &str, rest: &str) -> String {
    let mut chars = rest.char_indices().skip(1);
    let mut end = rest.len();
    while let Some((i, c)) = chars.next() {
        match c {
            '\\' => {
                chars.next();
            }
            '"' => {
                end = i + 1;
                break;
            }
            _ => {}
        }
    }
    let after = &rest[end..];
    let axis = after.split_whitespace().next().unwrap_or("");
    let build = after
        .split_once(":build ")
        .and_then(|(_, b)| string_literal(b.trim_start()))
        .unwrap_or_default();
    format!("{subject}\t{axis}\t{build}")
}

/// The source a producer reads its subjects from: the file itself, and every
/// file it splices with `include-file`, resolved against the including file.
fn producer_source(path: &Path) -> String {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| panic!("read the producer {}: {e}", path.display()));
    let dir = path.parent().expect("a producer has a directory");
    let mut out = text.clone();
    for line in text.lines() {
        if let Some(rest) = line.trim_start().strip_prefix("(include-file ") {
            if let Some(included) = string_literal(rest) {
                out.push('\n');
                out.push_str(&producer_source(&dir.join(included)));
            }
        }
    }
    out
}

/// Every producer on the ratchet: the leak dashboards, each residue test that
/// moved its window and its ceiling into a ledger, and the tool producers
/// under `tests/ratchet`.
const PRODUCERS: &[&str] = &[
    "tests/impl/oracle.lisp",
    "tests/impl/plumb.lisp",
    "tests/impl/h2-stress-scoped.lisp",
    "tests/impl/region-page-recycle.lisp",
    "tests/impl/region-macro-id-recycle.lisp",
    "tests/impl/region-collector-arg-move.lisp",
    "tests/impl/resource.lisp",
    "tests/ratchet/audit.lisp",
    "tests/ratchet/valgrind.lisp",
    RUNNER,
];

#[test]
fn each_producer_on_the_ratchet_has_a_ledger() {
    let producers: Vec<String> = ledgers().into_iter().map(|l| l.producer).collect();
    for want in PRODUCERS {
        assert!(
            producers.iter().any(|p| p == want),
            "tests/ledger holds a ledger for {want}; producers: {producers:?}"
        );
    }
}

#[test]
fn every_ledger_row_names_a_subject_its_producer_reads() {
    // A subject is a string literal in the producer's source, so a row whose
    // subject appears nowhere in that source names a probe that is not
    // there. The instrument's own live-growth rows are named by the
    // instrument, not the producer.
    let mut stale = Vec::new();
    let charged = crate::common::charge_files();
    for ledger in ledgers() {
        if ledger.producer == RUNNER {
            for subject in &ledger.subjects {
                if !charged.contains(subject) {
                    stale.push(format!(
                        "{} -> {subject:?}, which the charge pass never runs",
                        ledger.file.display()
                    ));
                }
            }
            continue;
        }
        let source = producer_source(&repo_root().join(&ledger.producer));
        for subject in &ledger.subjects {
            if subject.ends_with(" gauge (live-growth)") {
                continue;
            }
            let literal = format!("\"{}\"", subject.replace('\\', "\\\\").replace('"', "\\\""));
            if !source.contains(&literal) {
                stale.push(format!(
                    "{} -> {:?}, which {} never names",
                    ledger.file.display(),
                    subject,
                    ledger.producer
                ));
            }
        }
    }
    assert!(
        stale.is_empty(),
        "ledger rows whose subject their producer never reads:\n  {}",
        stale.join("\n  ")
    );
}

#[test]
fn no_row_of_one_build_is_in_two_files_of_one_producer() {
    // A producer's rows may span several files, as the runner's do. The row
    // reader keeps one row per subject, axis and build, so a second copy in
    // another file would leave the verdict to whichever file it read last.
    let mut seen = std::collections::BTreeMap::new();
    let mut twice = Vec::new();
    for ledger in ledgers() {
        let at = ledger.file.display().to_string();
        for key in &ledger.keys {
            let whose = format!("{}\t{key}", ledger.producer);
            if let Some(first) = seen.insert(whose, at.clone()) {
                twice.push(format!("{key:?} in {first} and {at}"));
            }
        }
    }
    assert!(
        twice.is_empty(),
        "rows written twice:\n  {}",
        twice.join("\n  ")
    );
}
