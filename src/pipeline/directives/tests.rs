// audited: 2026-09-21
// The strip is total, the validation loud: each case pins one clause of
// docs/versioning.md's declaration grammar.
use super::*;
use crate::reader::read_syntax_all;

fn forms_of(source: &str) -> (crate::syntax::SyntaxHeap, Vec<Syntax>) {
    let mut home = crate::syntax::SyntaxHeap::new();
    let forms = read_syntax_all(home.arena(), source, "<directives>").expect("reads");
    (home, forms)
}

#[test]
fn declarations_are_stripped_and_recorded() {
    let (_home, mut forms) = forms_of(
        "(elle/version \"1.2.3\")\n\
         (elle/migration 2 \"summary\" (rename a b))\n\
         (elle/migration 3 (warn f \"louder\"))\n\
         (def x 1)",
    );
    let meta = extract_semver_directives(&mut forms).expect("extracts");
    assert_eq!(meta.version.as_deref(), Some("1.2.3"));
    assert_eq!(meta.migrations, vec![2, 3]);
    assert_eq!(forms.len(), 1, "only (def x 1) remains");
}

#[test]
fn the_query_call_passes_through() {
    let (_home, mut forms) = forms_of("(elle/version)");
    let meta = extract_semver_directives(&mut forms).expect("extracts");
    assert_eq!(meta.version, None);
    assert_eq!(forms.len(), 1, "the 0-ary query is not a declaration");
}

#[test]
fn a_file_without_declarations_is_untouched() {
    let (_home, mut forms) = forms_of("(def x 1)\n(+ x 2)");
    let meta = extract_semver_directives(&mut forms).expect("extracts");
    assert_eq!(meta, SemverMeta::default());
    assert_eq!(forms.len(), 2);
}

/// Each malformed declaration refuses with a message naming the clause.
#[test]
fn malformed_declarations_refuse() {
    for (source, why) in [
        (
            "(elle/version \"1.0.0\") (elle/version \"1.0.1\")",
            "duplicate version",
        ),
        ("(elle/version (string \"1\"))", "non-literal version"),
        ("(elle/version \"1\" \"2\")", "two version arguments"),
        ("(elle/migration 0 (rename a b))", "major zero"),
        ("(elle/migration -1 (rename a b))", "negative major"),
        ("(elle/migration 2)", "no rules"),
        ("(elle/migration 2 \"summary\")", "summary but no rules"),
        (
            "(elle/migration 2 (rename a b)) (elle/migration 2 (rename c d))",
            "duplicate major",
        ),
        ("(elle/migration 2 (explode f))", "unknown rule"),
        ("(elle/migration 2 (rename a))", "rename arity"),
        ("(elle/migration 2 (rename a \"b\"))", "rename non-symbol"),
        ("(elle/migration 2 (replace f (g $1)))", "replace non-call"),
        ("(elle/migration 2 (remove gone))", "remove without message"),
        (
            "(elle/migration 2 (warn f loud))",
            "warn non-string message",
        ),
        ("(elle/migration 2 rename)", "rule is not a list"),
    ] {
        let (_home, mut forms) = forms_of(source);
        assert!(
            extract_semver_directives(&mut forms).is_err(),
            "{why}: {source} should refuse"
        );
    }
}
