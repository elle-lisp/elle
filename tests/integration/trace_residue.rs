// audited: 2026-09-07
// `--trace=residue` prints the teardown leak dump (docs/impl/region/
// diagnostics.md § Diagnostics): after the sweep, one `arena/dump` line per
// surviving region and one `[trace:residue] edge a -> b` line per cross-region
// reference among them.

use std::process::Command;

fn elle_binary() -> &'static str {
    env!("CARGO_BIN_EXE_elle")
}

#[test]
fn residue_trace_dumps_surviving_regions_and_edges() {
    let dir = crate::common::ScratchDir::new("trace-residue");
    let script = dir.join("script.lisp");
    std::fs::write(&script, "(+ 1 2)\n").expect("write script");
    let out = Command::new(elle_binary())
        .arg("--trace=residue")
        .arg(&script)
        .output()
        .expect("run elle");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(
        out.status.success(),
        "elle --trace=residue failed; stderr:\n{}",
        stderr
    );

    // The header states the count the dump then itemizes.
    let header = stderr
        .lines()
        .find(|l| l.starts_with("[trace:residue] live regions after teardown: "))
        .unwrap_or_else(|| panic!("missing [trace:residue] header; stderr:\n{}", stderr));
    let count: usize = header
        .rsplit(' ')
        .next()
        .unwrap()
        .parse()
        .unwrap_or_else(|e| panic!("unparseable residue count in {:?}: {}", header, e));

    // One region line per counted survivor. Zero residue — the end-state
    // target — prints a bare header and no region lines, and this test must
    // keep passing on that day.
    let region_lines = stderr
        .lines()
        .filter(|l| l.trim_start().starts_with("region ") && l.contains(" rc="))
        .count();
    assert_eq!(
        region_lines, count,
        "header says {} regions but {} region lines follow; stderr:\n{}",
        count, region_lines, stderr
    );

    // Edge lines only ever name counted regions.
    if count == 0 {
        assert!(
            !stderr.contains("[trace:residue] edge "),
            "edges printed with zero residue; stderr:\n{}",
            stderr
        );
    }
}

#[test]
fn residue_trace_is_silent_when_unset() {
    let dir = crate::common::ScratchDir::new("trace-residue-off");
    let script = dir.join("script.lisp");
    std::fs::write(&script, "(+ 1 2)\n").expect("write script");
    let out = Command::new(elle_binary())
        .arg(&script)
        .output()
        .expect("run elle");
    let stderr = String::from_utf8_lossy(&out.stderr);
    assert!(out.status.success(), "elle failed; stderr:\n{}", stderr);
    assert!(
        !stderr.contains("[trace:residue]"),
        "residue dump printed without --trace=residue; stderr:\n{}",
        stderr
    );
}
