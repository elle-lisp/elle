// audited: 2026-09-21
// The fixpoint and merge semantics of library rules, pinned at the Rust
// layer; tests/elle/compile-apply-rules.lisp pins the primitive above.
// docs/analysis/portrait.md

use super::*;

fn renames(pairs: &[(&str, &str)]) -> HashMap<String, String> {
    pairs
        .iter()
        .map(|(a, b)| (a.to_string(), b.to_string()))
        .collect()
}

#[test]
fn a_rename_edits_every_exact_occurrence() {
    let rules = LibraryRules {
        renames: renames(&[("old", "new")]),
        ..Default::default()
    };
    let out = apply_library_rules("(old 1) (older 2) (old 3)", "<t>", &rules).unwrap();
    assert_eq!(out.source, "(new 1) (older 2) (new 3)");
    assert_eq!(out.count, 2);
    assert!(out.reports.is_empty());
}

#[test]
fn a_replace_matches_only_its_arity_and_interpolates_source() {
    let rules = LibraryRules {
        replaces: vec![("m:bump".to_string(), 2, "(m:increment $2 $1)".to_string())],
        ..Default::default()
    };
    let out = apply_library_rules("(m:bump a (g b))", "<t>", &rules).unwrap();
    assert_eq!(out.source, "(m:increment (g b) a)");
    let same = apply_library_rules("(m:bump a)", "<t>", &rules).unwrap();
    assert_eq!(same.source, "(m:bump a)");
}

#[test]
fn the_fixpoint_reaches_nested_and_renamed_arguments() {
    let rules = LibraryRules {
        renames: renames(&[("old", "new")]),
        replaces: vec![("wrap".to_string(), 1, "(w $1)".to_string())],
        ..Default::default()
    };
    let out = apply_library_rules("(wrap old)", "<t>", &rules).unwrap();
    assert_eq!(out.source, "(w new)");

    let nested = LibraryRules {
        replaces: vec![("m:bump".to_string(), 2, "(m:increment $2 $1)".to_string())],
        ..Default::default()
    };
    let out = apply_library_rules("(m:bump (m:bump a b) c)", "<t>", &nested).unwrap();
    assert_eq!(out.source, "(m:increment c (m:increment b a))");
}

#[test]
fn a_self_referential_template_stops_loudly() {
    let rules = LibraryRules {
        replaces: vec![("f".to_string(), 1, "(f $1)".to_string())],
        ..Default::default()
    };
    let err = apply_library_rules("(f x)", "<t>", &rules).unwrap_err();
    assert!(err.contains("did not converge"), "got: {}", err);
}

#[test]
fn reports_name_each_occurrence_with_its_line_and_edit_nothing() {
    let rules = LibraryRules {
        reports: vec![("gone".to_string(), "use new".to_string())],
        ..Default::default()
    };
    let out = apply_library_rules("(gone 1)\n(x)\n(gone 2)", "<t>", &rules).unwrap();
    assert_eq!(out.source, "(gone 1)\n(x)\n(gone 2)");
    assert_eq!(out.count, 0);
    assert_eq!(out.reports.len(), 2);
    assert_eq!(out.reports[0].line, 1);
    assert_eq!(out.reports[1].line, 3);
    assert_eq!(out.reports[0].message, "use new");
}
