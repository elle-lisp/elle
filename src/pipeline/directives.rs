// audited: 2026-09-21
//! The library-versioning declarations: validate and strip the top-level
//! `(elle/version "X.Y.Z")` and `(elle/migration N ...)` forms.
//! docs/versioning.md

use crate::syntax::{Syntax, SyntaxKind};

/// What a file declared: its version, and the majors it ships migrations
/// for. The rules themselves stay in the source — the consumer of a
/// migration reads the file's forms, not the compiler's model.
#[derive(Debug, Default, PartialEq, Eq)]
pub struct SemverMeta {
    pub version: Option<String>,
    pub migrations: Vec<u64>,
}

/// Validate and remove every version and migration declaration from the
/// file's top-level forms. Runs after epoch extraction and before epoch
/// migration, so the declarations never meet a rewrite rule.
pub fn extract_semver_directives(forms: &mut Vec<Syntax>) -> Result<SemverMeta, String> {
    let mut meta = SemverMeta::default();
    let mut kept = Vec::with_capacity(forms.len());
    for form in std::mem::take(forms) {
        let SyntaxKind::List(items) = &form.kind else {
            kept.push(form);
            continue;
        };
        // The 1-ary literal form declares; the 0-ary call stays the
        // interpreter-version query and passes through untouched.
        if items.len() >= 2 && items[0].is_symbol("elle/version") {
            let SyntaxKind::String(version) = &items[1].kind else {
                return Err(format!(
                    "{}: (elle/version ...) takes one literal string",
                    form.span
                ));
            };
            if items.len() != 2 {
                return Err(format!(
                    "{}: (elle/version ...) takes one literal string",
                    form.span
                ));
            }
            if meta.version.is_some() {
                return Err(format!(
                    "{}: duplicate (elle/version); a file declares one version",
                    form.span
                ));
            }
            meta.version = Some(version.to_string());
            continue;
        }
        if !items.is_empty() && items[0].is_symbol("elle/migration") {
            let major = validate_migration(items, &form)?;
            if meta.migrations.contains(&major) {
                return Err(format!(
                    "{}: duplicate (elle/migration {}); one form per major",
                    form.span, major
                ));
            }
            meta.migrations.push(major);
            continue;
        }
        kept.push(form);
    }
    *forms = kept;
    Ok(meta)
}

/// The major a well-formed `(elle/migration N ...)` declares. The form
/// carries a positive major, an optional summary string, then at least
/// one rule from the vocabulary docs/versioning.md fixes.
fn validate_migration(items: &[Syntax], form: &Syntax) -> Result<u64, String> {
    let major = match items.get(1).map(|s| &s.kind) {
        Some(SyntaxKind::Int(n)) if *n > 0 => *n as u64,
        _ => {
            return Err(format!(
                "{}: (elle/migration N ...) needs a positive major",
                form.span
            ))
        }
    };
    let mut rules = &items[2..];
    if let Some(SyntaxKind::String(_)) = rules.first().map(|s| &s.kind) {
        rules = &rules[1..];
    }
    if rules.is_empty() {
        return Err(format!(
            "{}: (elle/migration {}) declares no rules",
            form.span, major
        ));
    }
    for rule in rules {
        validate_rule(rule)?;
    }
    Ok(major)
}

/// One rule against the vocabulary: rename, replace, remove, warn.
fn validate_rule(rule: &Syntax) -> Result<(), String> {
    let SyntaxKind::List(items) = &rule.kind else {
        return Err(format!("{}: a migration rule is a list", rule.span));
    };
    let head = items.first().and_then(|s| s.as_symbol()).unwrap_or("");
    let shape_ok = match head {
        "rename" => {
            items.len() == 3 && items[1].as_symbol().is_some() && items[2].as_symbol().is_some()
        }
        "replace" => {
            items.len() == 3
                && matches!(&items[1].kind, SyntaxKind::List(call)
                    if call.first().and_then(|s| s.as_symbol()).is_some())
        }
        "remove" | "warn" => {
            items.len() == 3
                && items[1].as_symbol().is_some()
                && matches!(items[2].kind, SyntaxKind::String(_))
        }
        _ => {
            return Err(format!(
                "{}: unknown migration rule '{}'; the vocabulary is \
                 rename, replace, remove, warn",
                rule.span, head
            ))
        }
    };
    if !shape_ok {
        return Err(format!(
            "{}: malformed ({} ...) migration rule",
            rule.span, head
        ));
    }
    Ok(())
}

#[cfg(test)]
mod tests;
