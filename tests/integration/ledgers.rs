// audited: 2026-09-30
// The committed ledgers: every producer on the ratchet has one, every row of
// one names a subject its producer reads, and the root the ledger sits under
// is the tree the binary was built in.
//
// docs/ratchet.md
//
// The counter-factual: a row outlives the probe that read it, and `missing`
// only says so once the producer runs, which for a dashboard is a minute into
// the implementation suite. These read the ledgers as text and fail in a
// second.

use std::path::{Path, PathBuf};
use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

fn repo_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

/// One committed ledger: the producer it answers for, and the subject of every
/// row in it.
struct Ledger {
    file: PathBuf,
    producer: String,
    subjects: Vec<String>,
}

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
        for line in text.lines() {
            if let Some(rest) = line.strip_prefix("(producer ") {
                producer = string_literal(rest);
            } else if let Some(rest) = line.strip_prefix('[') {
                if let Some(subject) = string_literal(rest) {
                    subjects.push(subject);
                }
            }
        }
        let producer = producer.unwrap_or_else(|| panic!("{} names no producer", file.display()));
        out.push(Ledger {
            file,
            producer,
            subjects,
        });
    }
    out
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

/// Every corpus file on the ratchet: the leak dashboards, and each residue
/// test that moved its window and its ceiling into a ledger.
const PRODUCERS: &[&str] = &[
    "tests/impl/oracle.lisp",
    "tests/impl/plumb.lisp",
    "tests/impl/h2-stress-scoped.lisp",
    "tests/impl/region-page-recycle.lisp",
    "tests/impl/region-macro-id-recycle.lisp",
    "tests/impl/region-collector-arg-move.lisp",
    "tests/impl/resource.lisp",
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
    for ledger in ledgers() {
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
fn the_root_is_the_tree_the_binary_was_built_in() {
    let out = Command::new(elle_binary())
        .args(["-e", "(println (elle/root))"])
        .env_remove("RUST_MIN_STACK")
        .output()
        .expect("run elle -e");
    let printed = String::from_utf8_lossy(&out.stdout).trim().to_string();
    let got = Path::new(&printed).canonicalize().expect("the root exists");
    let want = repo_root().canonicalize().expect("the manifest dir exists");
    assert_eq!(got, want, "elle/root names the repository root");
}
