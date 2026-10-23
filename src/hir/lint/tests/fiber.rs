// audited: 2026-10-21
//! The two hints a literal `fiber/new` gets: a call its deny set will deny, and a signal that will pass its mask.
//!
//! docs/signals/capabilities.md
//! docs/cookbook/lint-rules.md

use super::lint_rule;
use crate::lint::diagnostics::{Diagnostic, Severity};

const DENIED: &str = "denied-fiber-call";
const PASSES: &str = "fiber-signal-passes";

/// A body that may raise `:fs` and `:error`, and nothing else: `file/stat`
/// declares exactly those two.
const READS: &str = "(fn [] (file/stat \"x\"))";

/// The one hint of `rule` that `source` gets. Panics unless there is exactly one.
fn the_hint(source: &str, rule: &str) -> Diagnostic {
    let mut hints = lint_rule(source, rule);
    assert_eq!(hints.len(), 1, "{source}: one {rule} hint, got {hints:?}");
    hints.remove(0)
}

/// Assert that `source` gets no hint of `rule`.
fn no_hint(source: &str, rule: &str) {
    let hints = lint_rule(source, rule);
    assert!(hints.is_empty(), "{source}: no {rule} hint, got {hints:?}");
}

#[test]
fn a_bit_the_deny_set_withholds_is_a_denied_call() {
    let source = format!("(fiber/new {READS} |:fs :error| :deny |:fs|)");
    let hint = the_hint(&source, DENIED);
    assert_eq!(hint.code, "I001");
    assert_eq!(hint.severity, Severity::Info, "a denial is not a fault");
    assert!(
        hint.message.contains(":fs"),
        "names the bit: {}",
        hint.message
    );
    // The mask names both bits, so nothing passes it.
    no_hint(&source, PASSES);
}

#[test]
fn a_bit_the_mask_does_not_name_passes_the_parent() {
    let source = format!("(fiber/new {READS} |:error|)");
    let hint = the_hint(&source, PASSES);
    assert_eq!(hint.code, "I002");
    assert_eq!(
        hint.severity,
        Severity::Info,
        "a passing signal is not a fault"
    );
    assert!(
        hint.message.contains(":fs"),
        "names the bit: {}",
        hint.message
    );
    assert!(
        !hint.message.contains(":error"),
        "the mask catches :error: {}",
        hint.message
    );
    no_hint(&source, DENIED);
}

#[test]
fn a_denied_bit_the_mask_does_not_catch_gets_both_hints() {
    // The denial raises the withheld bit, and the mask does not name it, so the
    // denial passes the parent too.
    let source = format!("(fiber/new {READS} |:error| :deny |:fs|)");
    the_hint(&source, DENIED);
    the_hint(&source, PASSES);
}

#[test]
fn a_keyword_mask_is_a_literal_mask() {
    no_hint("(fiber/new (fn [] (yield 1)) :yield)", PASSES);
    the_hint("(fiber/new (fn [] (yield 1)) :error)", PASSES);
}

#[test]
fn the_alias_is_the_same_primitive() {
    the_hint(&format!("(fiber {READS} |:error|)"), PASSES);
}

#[test]
fn a_body_the_compiler_cannot_see_gets_no_hint() {
    let source = "(fn [body] (fiber/new body |:error| :deny |:fs|))";
    no_hint(source, DENIED);
    no_hint(source, PASSES);
}

#[test]
fn a_computed_mask_or_deny_set_gets_no_hint_on_its_side() {
    no_hint(&format!("(fn [m] (fiber/new {READS} m))"), PASSES);
    no_hint(
        &format!("(fn [d] (fiber/new {READS} |:fs :error| :deny d))"),
        DENIED,
    );
}

#[test]
fn a_binding_that_shadows_the_primitive_gets_no_hint() {
    // Counter-factual: a rule that matched the callee by its spelling would
    // read this parameter as `fiber/new`'s alias.
    no_hint(&format!("(fn [fiber] (fiber {READS} |:error|))"), PASSES);
}

#[test]
fn a_hint_names_its_enclosing_function() {
    let hint = the_hint(
        &format!("(defn spawn [] (fiber/new {READS} |:error|))"),
        PASSES,
    );
    assert_eq!(hint.function.as_deref(), Some("spawn"));
}

#[test]
fn a_hint_is_not_a_warning() {
    let (mut symbols, mut vm) = super::setup();
    let source = format!("(fiber/new {READS} |:error| :deny |:fs|)");
    let analysis =
        super::analyze(&source, &mut symbols, &mut vm, "<test>").expect("source should analyze");
    let mut linter = super::HirLinter::new();
    linter.lint(&analysis.hir, &symbols, &analysis.arena);
    assert_eq!(linter.diagnostics().len(), 2, "{:?}", linter.diagnostics());
    assert!(!linter.has_warnings(), "{:?}", linter.diagnostics());
}
