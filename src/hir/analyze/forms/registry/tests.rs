//! Unit tests (`super` is the parent impl module).

use super::*;

/// Names and aliases must be unique, and every user-facing form must
/// carry doc text — the registry is the single source the doc system
/// and reserved-word list trust.
#[test]
fn registry_names_unique_and_documented() {
    let mut seen = std::collections::HashSet::new();
    for name in all_names() {
        assert!(seen.insert(name), "duplicate special-form name '{}'", name);
    }
    for form in SPECIAL_FORMS {
        if form.internal {
            continue;
        }
        assert!(
            !form.doc.is_empty(),
            "special form '{}' lacks doc text",
            form.name
        );
        assert!(
            !form.example.is_empty(),
            "special form '{}' lacks an example",
            form.name
        );
    }
}

/// Drift guard: `(doc "<form>")` must work for every user-facing
/// special form and alias. Before the registry, the doc table was a
/// second hand-written list that silently lagged the analyzer (the
/// signal-system forms had no docs at all).
#[test]
fn docs_cover_every_special_form() {
    let mut docs = std::collections::HashMap::new();
    crate::primitives::docs::register_builtin_docs(&mut docs);
    for form in SPECIAL_FORMS {
        if form.internal {
            continue;
        }
        for name in std::iter::once(form.name).chain(form.aliases.iter().copied()) {
            assert!(
                docs.contains_key(name),
                "special form '{}' has no doc entry",
                name
            );
        }
    }
}
