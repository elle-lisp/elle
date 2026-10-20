// audited: 2026-10-20
// The sources the audit queue walks: every queued file that is not a document, which the size producer reads.
//
// docs/impl/audit.md
//
// The size producer prices exactly these files (tests/ratchet/overview.md), so
// the list is the one place the size ledger and the queue agree on what is
// exempt.

use std::path::Path;
use std::process::Command;

use crate::common::ScratchDir;

/// Write `body` at `rel` under `root`, creating its directories.
fn write(root: &Path, rel: &str, body: &str) {
    let path = root.join(rel);
    std::fs::create_dir_all(path.parent().expect("has a parent")).expect("create fixture dir");
    std::fs::write(&path, body).expect("write fixture file");
}

/// `scripts/audit --root ROOT --sources`, one path per line.
fn sources(root: &Path) -> Vec<String> {
    let script = std::path::PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("scripts/audit");
    let out = Command::new(script)
        .args(["--root", root.to_str().expect("utf-8 path"), "--sources"])
        .output()
        .expect("run scripts/audit");
    assert!(
        out.status.success(),
        "scripts/audit --sources failed with {:?}:\n{}",
        out.status.code(),
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8(out.stdout)
        .expect("utf-8 output")
        .lines()
        .map(str::to_string)
        .collect()
}

#[test]
fn sources_lists_every_queued_file_that_is_not_a_document() {
    // The counter-factual: a list that dropped the scripts, or a Lisp file,
    // leaves that source off the size ledger, and it grows with no row to
    // stop it.
    let t = ScratchDir::new("audit-sources");
    write(
        t.path(),
        "DOCUMENTATION.md",
        "# Policy\n\n<!-- audited: 2026-06-01 -->\n",
    );
    write(t.path(), "src/main.rs", "fn main() {}\n");
    write(t.path(), "lib/http.lisp", "(def x 1)\n");
    write(t.path(), "scripts/tool", "#!/bin/sh\n");
    write(t.path(), "docs/guide.md", "# Guide\n");

    assert_eq!(
        sources(t.path()),
        ["lib/http.lisp", "scripts/tool", "src/main.rs"],
        "the three sources, sorted, and neither document"
    );
}

#[test]
fn sources_leaves_out_what_the_queue_exempts() {
    // The counter-factual: a second copy of the exemptions drifts from the
    // queue's, and a vendored table or the standard library lands on the size
    // ledger as a debt no remedy can pay.
    let t = ScratchDir::new("audit-sources-exempt");
    write(t.path(), "ours.rs", "fn f() {}\n");
    write(t.path(), "vendor/LICENSE-MIT", "MIT\n");
    write(t.path(), "vendor/tables.rs", "const T: [u8; 1] = [0];\n");
    write(t.path(), "src/stdlib.lisp", "(def x 1)\n");
    write(t.path(), "src/lir/code/instr.rs", "enum InstrRef {}\n");
    write(t.path(), "Makefile", "all:\n");
    write(t.path(), "plugins/p.rs", "fn p() {}\n");

    assert_eq!(
        sources(t.path()),
        ["ours.rs"],
        "every exempt file stays off the list"
    );
}
