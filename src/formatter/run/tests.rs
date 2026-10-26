// audited: 2026-10-26
//! Tests of `elle fmt`: the epoch rewrite, the left margin, and the exit status of --check.
//!
//! docs/fmt.md

use super::*;
use crate::epoch::CURRENT_EPOCH;
use crate::scratch::ScratchDir;

#[test]
fn test_fmt_upgrades_epoch() {
    let config = FormatterConfig::default();
    let input = "(elle/epoch 0)\n(assert-true x \"test\")\n";
    let result = rewrite_and_format(input, "<test>", &config).unwrap();
    assert!(
        result.contains(&format!("(elle/epoch {})", CURRENT_EPOCH)),
        "should upgrade epoch tag, got: {:?}",
        result
    );
    assert!(
        !result.contains("assert-true"),
        "old symbol should be rewritten, got: {:?}",
        result
    );
}

#[test]
fn test_fmt_current_epoch_unchanged() {
    let config = FormatterConfig::default();
    let input = format!("(elle/epoch {})\n(println \"hello\")\n", CURRENT_EPOCH);
    let result = rewrite_and_format(&input, "<test>", &config).unwrap();
    assert_eq!(result, input, "current-epoch file should be unchanged");
}

#[test]
fn test_fmt_no_epoch_gets_one() {
    let config = FormatterConfig::default();
    let input = "(println \"hello\")\n";
    let result = rewrite_and_format(input, "<test>", &config).unwrap();
    assert!(
        result.contains(&format!("(elle/epoch {})", CURRENT_EPOCH)),
        "should inject epoch tag, got: {:?}",
        result
    );
}

#[test]
fn test_fmt_epoch_upgrade_idempotent() {
    let config = FormatterConfig::default();
    let input = "(elle/epoch 0)\n(assert-true x \"test\")\n";
    let first = rewrite_and_format(input, "<test>", &config).unwrap();
    let second = rewrite_and_format(&first, "<test>", &config).unwrap();
    assert_eq!(first, second, "rewrite+format must be idempotent");
}

#[test]
fn test_no_epoch_skips_injection() {
    let config = FormatterConfig::default();
    let opts = FmtOpts {
        no_epoch: true,
        preserve_margin: false,
    };
    let input = "(defn foo [x]\n  (+ x 1))\n";
    let result = do_format(input, "<test>", &config, &opts).unwrap();
    assert!(
        !result.contains("elle/epoch"),
        "--no-epoch should not inject epoch, got: {:?}",
        result
    );
}

#[test]
fn test_preserve_left_margin() {
    let config = FormatterConfig::default();
    let opts = FmtOpts {
        no_epoch: true,
        preserve_margin: true,
    };
    let input = "    (defn foo [x]\n      (+ x 1))\n";
    let result = do_format(input, "<test>", &config, &opts).unwrap();
    assert!(
        result.starts_with("    (defn foo [x]"),
        "should preserve 4-space margin, got: {:?}",
        result
    );
    // Body should be margin + 2 indent = 6 spaces
    let lines: Vec<&str> = result.lines().collect();
    assert!(
        lines[1].starts_with("      "),
        "body should be at margin+2, got: {:?}",
        lines[1]
    );
}

#[test]
fn test_preserve_left_margin_idempotent() {
    let config = FormatterConfig::default();
    let opts = FmtOpts {
        no_epoch: true,
        preserve_margin: true,
    };
    let input = "        (defn foo [x]\n          (+ x 1))\n";
    let first = do_format(input, "<test>", &config, &opts).unwrap();
    let second = do_format(&first, "<test>", &config, &opts).unwrap();
    assert_eq!(first, second, "plm must be idempotent");
}

#[test]
fn test_strip_left_margin_basic() {
    let (stripped, margin) = strip_left_margin("    (foo)\n      (bar)\n");
    assert_eq!(margin, "    ");
    assert_eq!(stripped, "(foo)\n  (bar)\n");
}

#[test]
fn test_strip_left_margin_zero() {
    let (stripped, margin) = strip_left_margin("(foo)\n  (bar)\n");
    assert_eq!(margin, "");
    assert_eq!(stripped, "(foo)\n  (bar)\n");
}

#[test]
fn test_strip_left_margin_blank_lines() {
    let (stripped, margin) = strip_left_margin("    (foo)\n\n    (bar)\n");
    assert_eq!(margin, "    ");
    assert!(stripped.contains("\n\n"), "blank lines should be preserved");
}

// ── --check over several files ──────────────────────────────

/// `(def x [[…[1]…]])` with `depth` brackets, as the formatter lays it out, so
/// `--check` finds nothing to change and reads only its columns.
fn nested(depth: usize) -> String {
    let src = format!("(def x {}1{})\n", "[".repeat(depth), "]".repeat(depth));
    format_code(&src, &FormatterConfig::default()).unwrap()
}

/// The exit status of `elle fmt --check --no-epoch` over `files`, in order.
fn check(files: &[&std::path::Path]) -> i32 {
    let mut args = vec!["--check".to_string(), "--no-epoch".to_string()];
    args.extend(
        files
            .iter()
            .map(|f| f.to_str().expect("a UTF-8 path").to_string()),
    );
    run(&args)
}

// A file whose delimiters pass column 60 but not column 80 warns, and a
// warning alone leaves the exit status at 0. This is the control for the two
// tests below: their last file warns and does nothing else.
#[test]
fn a_file_that_only_warns_passes_the_check() {
    let dir = ScratchDir::new("fmt-check-warns");
    let warns = dir.join("warns.lisp");
    std::fs::write(&warns, nested(70)).unwrap();
    assert_eq!(check(&[&warns]), 0);
}

// The counter-factual assigns each file's column verdict to the run's error
// flag rather than adding it, so a later file that only warns clears an
// earlier file's column error, and the run exits 0.
#[test]
fn a_later_warning_keeps_an_earlier_column_error() {
    let dir = ScratchDir::new("fmt-check-breaks");
    let breaks = dir.join("breaks.lisp");
    let warns = dir.join("warns.lisp");
    std::fs::write(&breaks, nested(90)).unwrap();
    std::fs::write(&warns, nested(70)).unwrap();
    assert_eq!(
        check(&[&breaks, &warns]),
        1,
        "a warning in the second file cleared the first file's error"
    );
}

// The same assignment clears a file that could not be read.
#[test]
fn a_later_warning_keeps_an_unreadable_file() {
    let dir = ScratchDir::new("fmt-check-missing");
    let missing = dir.join("missing.lisp");
    let warns = dir.join("warns.lisp");
    std::fs::write(&warns, nested(70)).unwrap();
    assert_eq!(
        check(&[&missing, &warns]),
        1,
        "a warning in the second file cleared the first file's read error"
    );
}
