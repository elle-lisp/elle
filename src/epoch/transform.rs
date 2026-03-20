//! Syntax tree migration transformer.
//!
//! Walks a syntax tree and applies epoch migration rules in a single pass.
//! Renames are O(1) per symbol node (hash lookup). The tree is walked once
//! regardless of how many epochs are being crossed.

use crate::syntax::{Syntax, SyntaxKind};
use std::collections::HashMap;

use super::rules::{collapsed_renames, removals_in_range};

/// Migrate syntax forms from `from_epoch` to `to_epoch`.
///
/// Applies all renames in one pass using a collapsed lookup table.
/// Returns the number of nodes rewritten. Returns `Err` if a removed
/// form is encountered.
pub fn migrate(forms: &mut [Syntax], from_epoch: u64, to_epoch: u64) -> Result<usize, String> {
    let renames = collapsed_renames(from_epoch, to_epoch);
    let removals = removals_in_range(from_epoch, to_epoch);

    if renames.is_empty() && removals.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for form in forms.iter_mut() {
        count += rewrite_node(form, &renames, &removals)?;
    }
    Ok(count)
}

/// Recursively rewrite a single syntax node.
fn rewrite_node(
    syntax: &mut Syntax,
    renames: &HashMap<&str, &str>,
    removals: &HashMap<&str, &str>,
) -> Result<usize, String> {
    let mut count = 0;

    match &mut syntax.kind {
        SyntaxKind::Symbol(name) => {
            if let Some(msg) = removals.get(name.as_str()) {
                return Err(format!(
                    "epoch migration error at {}: `{}` has been removed — {}",
                    syntax.span, name, msg
                ));
            }
            if let Some(new_name) = renames.get(name.as_str()) {
                *name = new_name.to_string();
                count += 1;
            }
        }

        SyntaxKind::List(items)
        | SyntaxKind::Array(items)
        | SyntaxKind::ArrayMut(items)
        | SyntaxKind::Struct(items)
        | SyntaxKind::StructMut(items)
        | SyntaxKind::Set(items)
        | SyntaxKind::SetMut(items) => {
            for item in items.iter_mut() {
                count += rewrite_node(item, renames, removals)?;
            }
        }

        // Don't rewrite inside quotes — quoted symbols are data.
        SyntaxKind::Quote(_) => {}

        // Quasiquote templates construct code; rewrite so generated
        // code uses current names.
        SyntaxKind::Quasiquote(inner)
        | SyntaxKind::Unquote(inner)
        | SyntaxKind::UnquoteSplicing(inner)
        | SyntaxKind::Splice(inner) => {
            count += rewrite_node(inner, renames, removals)?;
        }

        // Atoms — nothing to rewrite.
        SyntaxKind::Nil
        | SyntaxKind::Bool(_)
        | SyntaxKind::Int(_)
        | SyntaxKind::Float(_)
        | SyntaxKind::Keyword(_)
        | SyntaxKind::String(_)
        | SyntaxKind::SyntaxLiteral(_) => {}
    }

    Ok(count)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::syntax::{Span, Syntax, SyntaxKind};

    fn sym(name: &str) -> Syntax {
        Syntax::new(SyntaxKind::Symbol(name.to_string()), Span::synthetic())
    }

    fn int(n: i64) -> Syntax {
        Syntax::new(SyntaxKind::Int(n), Span::synthetic())
    }

    fn list(items: Vec<Syntax>) -> Syntax {
        Syntax::new(SyntaxKind::List(items), Span::synthetic())
    }

    #[test]
    fn test_rename_symbol() {
        let renames: HashMap<&str, &str> = [("foo", "bar")].into_iter().collect();
        let removals = HashMap::new();

        let mut form = sym("foo");
        let count = rewrite_node(&mut form, &renames, &removals).unwrap();

        assert_eq!(count, 1);
        assert_eq!(form.as_symbol(), Some("bar"));
    }

    #[test]
    fn test_rename_in_list() {
        let renames: HashMap<&str, &str> = [("old", "new")].into_iter().collect();
        let removals = HashMap::new();

        let mut form = list(vec![sym("old"), int(1), sym("old")]);
        let count = rewrite_node(&mut form, &renames, &removals).unwrap();

        assert_eq!(count, 2);
        if let SyntaxKind::List(items) = &form.kind {
            assert_eq!(items[0].as_symbol(), Some("new"));
            assert_eq!(items[2].as_symbol(), Some("new"));
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_no_rewrite_inside_quote() {
        let renames: HashMap<&str, &str> = [("foo", "bar")].into_iter().collect();
        let removals = HashMap::new();

        let mut form = Syntax::new(SyntaxKind::Quote(Box::new(sym("foo"))), Span::synthetic());
        let count = rewrite_node(&mut form, &renames, &removals).unwrap();

        assert_eq!(count, 0);
        if let SyntaxKind::Quote(inner) = &form.kind {
            assert_eq!(inner.as_symbol(), Some("foo"));
        }
    }

    #[test]
    fn test_rewrite_inside_quasiquote() {
        let renames: HashMap<&str, &str> = [("foo", "bar")].into_iter().collect();
        let removals = HashMap::new();

        let mut form = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(sym("foo"))),
            Span::synthetic(),
        );
        let count = rewrite_node(&mut form, &renames, &removals).unwrap();

        assert_eq!(count, 1);
        if let SyntaxKind::Quasiquote(inner) = &form.kind {
            assert_eq!(inner.as_symbol(), Some("bar"));
        }
    }

    #[test]
    fn test_removal_errors() {
        let renames = HashMap::new();
        let removals: HashMap<&str, &str> =
            [("gone", "use replacement instead")].into_iter().collect();

        let mut form = sym("gone");
        let result = rewrite_node(&mut form, &renames, &removals);

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("has been removed"));
    }

    #[test]
    fn test_no_changes_no_rules() {
        let renames = HashMap::new();
        let removals = HashMap::new();

        let mut form = list(vec![sym("foo"), int(1)]);
        let count = rewrite_node(&mut form, &renames, &removals).unwrap();

        assert_eq!(count, 0);
    }

    #[test]
    fn test_migrate_empty_range() {
        let mut forms = vec![list(vec![sym("foo"), int(1)])];
        let count = migrate(&mut forms, 0, 0).unwrap();
        assert_eq!(count, 0);
    }
}
