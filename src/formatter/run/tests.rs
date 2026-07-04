//! Unit tests (`super` is the parent impl module).

use super::*;
use crate::epoch::CURRENT_EPOCH;

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
