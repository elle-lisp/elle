// audited: 2026-10-20
// The curve tool bins every source by tokens and by lines, and counts the ones whose header names three documents.
// tools/overview.md
// docs/analysis/budget.md
//
// The fixture's token counts were measured apart from the rig. Each source
// sits in a bin of its own, so a count off by one source shows in one row.
//
// The counter-factuals are in the headers. `b.rs` names one document twice
// and another in brackets, so a reader that counted names rather than
// distinct paths reads it as naming three. `c.lisp` names `./local.md` and
// `name.md`, which no header rule reads as a path. `e.rs` names a document on
// its thirteenth line, past the header a reader reads.

mod common;

use common::{audited_tree, repo, stderr, stdout};
use std::process::Command;

/// A Rust line of ten tokens.
const RUST: &str = "let walked = reach.charge(edge, pace);\n";

/// A Lisp line of eight tokens.
const LISP: &str = "(def walked (charge reach edge pace))\n";

/// Each bin with a source in it: the axis and width, the denominator, the
/// bin, its sources, how many name three documents, and that share.
const CURVE: &str = "\
tokens\t500\theaded\t0-499\t1\t0\t0.0
tokens\t500\theaded\t1000-1499\t1\t0\t0.0
tokens\t500\theaded\t3000-3499\t1\t1\t100.0
tokens\t500\theaded\t5500+\t1\t1\t100.0
tokens\t500\tevery\t0-499\t3\t0\t0.0
tokens\t500\tevery\t1000-1499\t1\t0\t0.0
tokens\t500\tevery\t2500-2999\t1\t0\t0.0
tokens\t500\tevery\t3000-3499\t1\t1\t100.0
tokens\t500\tevery\t5500+\t1\t1\t100.0
tokens\t1000\theaded\t0-999\t1\t0\t0.0
tokens\t1000\theaded\t1000-1999\t1\t0\t0.0
tokens\t1000\theaded\t3000-3999\t1\t1\t100.0
tokens\t1000\theaded\t5000+\t1\t1\t100.0
tokens\t1000\tevery\t0-999\t3\t0\t0.0
tokens\t1000\tevery\t1000-1999\t1\t0\t0.0
tokens\t1000\tevery\t2000-2999\t1\t0\t0.0
tokens\t1000\tevery\t3000-3999\t1\t1\t100.0
tokens\t1000\tevery\t5000+\t1\t1\t100.0
lines\t100\theaded\t0-99\t1\t0\t0.0
lines\t100\theaded\t100-199\t1\t0\t0.0
lines\t100\theaded\t300-399\t1\t1\t100.0
lines\t100\theaded\t500+\t1\t1\t100.0
lines\t100\tevery\t0-99\t3\t0\t0.0
lines\t100\tevery\t100-199\t1\t0\t0.0
lines\t100\tevery\t200-299\t1\t0\t0.0
lines\t100\tevery\t300-399\t1\t1\t100.0
lines\t100\tevery\t500+\t1\t1\t100.0
";

#[test]
fn the_curve_bins_each_source_and_counts_the_ones_naming_three_documents() {
    let t = audited_tree("budget");
    // 3,018 tokens and 303 lines, naming three documents.
    t.write(
        "src/a.rs",
        &format!(
            "// A call-out.\n// docs/a.md, docs/b.md\n// docs/c.md\n{}",
            RUST.repeat(300)
        ),
    );
    // 1,012 tokens and 101 lines, naming two.
    t.write(
        "src/b.rs",
        &format!("// docs/a.md docs/a.md (docs/b.md)\n{}", RUST.repeat(100)),
    );
    // 412 tokens and 52 lines, naming one.
    t.write(
        "lib/c.lisp",
        &format!("# lib/x.md\n# ./local.md name.md\n{}", LISP.repeat(50)),
    );
    // 7,015 tokens and 702 lines, naming three.
    t.write(
        "src/big.rs",
        &format!(
            "// A call-out.\n// docs/a.md docs/b.md docs/c.md\n{}",
            RUST.repeat(700)
        ),
    );
    // 2,600 tokens and 260 lines, with no header.
    t.write("src/d.rs", &RUST.repeat(260));
    // 153 tokens and 23 lines, naming none in its first twelve.
    t.write(
        "src/e.rs",
        &format!(
            "{}// docs/z.md\n{}",
            "// filler line\n".repeat(12),
            RUST.repeat(10)
        ),
    );
    // A document, which is no source.
    t.write(
        "docs/guide.md",
        "# Guide\n\n<!-- audited: 2026-10-19 -->\n\nA guide.\n",
    );

    let out = Command::new(env!("CARGO_BIN_EXE_elle-rig"))
        .current_dir(t.path())
        .arg(repo().join("tools/budget.lisp"))
        .output()
        .expect("spawn elle-rig");
    assert!(
        out.status.success(),
        "the curve tool runs green:\n{}",
        stderr(&out)
    );
    assert_eq!(stdout(&out), CURVE);
}
