//! Syntax tree migration transformer.
//!
//! Walks a syntax tree and applies epoch migration rules in a single pass.
//! Renames are O(1) per symbol node (hash lookup). Replacements match call
//! forms by head symbol and arity, then restructure using a template. The
//! tree is walked once regardless of how many epochs are being crossed.

use crate::syntax::{Span, Syntax, SyntaxKind};
use std::collections::HashMap;

use super::rules::{
    collapsed_renames, removals_in_range, replace_rules_in_range, unwrap_rules_in_range,
};

/// Migrate syntax forms from `from_epoch` to `to_epoch`.
///
/// Applies all renames and replacements in one pass. Returns the number
/// of nodes rewritten. Returns `Err` if a removed form is encountered.
pub fn migrate(forms: &mut [Syntax], from_epoch: u64, to_epoch: u64) -> Result<usize, String> {
    let renames = collapsed_renames(from_epoch, to_epoch);
    let removals = removals_in_range(from_epoch, to_epoch);
    let replaces = replace_rules_in_range(from_epoch, to_epoch);
    let unwraps = unwrap_rules_in_range(from_epoch, to_epoch);

    if renames.is_empty() && removals.is_empty() && replaces.is_empty() && unwraps.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for form in forms.iter_mut() {
        count += rewrite_node(form, &renames, &removals, &replaces, &unwraps)?;
    }
    Ok(count)
}

/// Recursively rewrite a single syntax node.
fn rewrite_node(
    syntax: &mut Syntax,
    renames: &HashMap<&str, &str>,
    removals: &HashMap<&str, &str>,
    replaces: &[(&str, usize, &str)],
    unwraps: &HashMap<&str, &str>,
) -> Result<usize, String> {
    let mut count = 0;

    // Check for Unwrap match: (symbol (fn [] body...)) → (begin body...)
    if let SyntaxKind::List(items) = &syntax.kind {
        if let Some(head_sym) = items.first().and_then(|s| s.as_symbol()) {
            if let Some(message) = unwraps.get(head_sym) {
                // Must be exactly 2 items: (symbol (fn [] body...))
                if items.len() == 2 {
                    if let SyntaxKind::List(lambda_items) = &items[1].kind {
                        // Check (fn [] body...) or (fn () body...)
                        let is_fn = lambda_items
                            .first()
                            .and_then(|s| s.as_symbol())
                            .is_some_and(|s| s == "fn");
                        let has_empty_params = lambda_items.get(1).is_some_and(|p| {
                            matches!(&p.kind, SyntaxKind::List(v) | SyntaxKind::Array(v) if v.is_empty())
                        });
                        if is_fn && has_empty_params && lambda_items.len() >= 3 {
                            let body: Vec<Syntax> = lambda_items[2..].to_vec();
                            let span = syntax.span.clone();
                            if body.len() == 1 {
                                syntax.kind = body.into_iter().next().unwrap().kind;
                            } else {
                                let mut begin_items = vec![Syntax::new(
                                    SyntaxKind::Symbol("begin".to_string()),
                                    span,
                                )];
                                begin_items.extend(body);
                                syntax.kind = SyntaxKind::List(begin_items);
                            }
                            count += 1;
                            count += rewrite_node(syntax, renames, removals, replaces, unwraps)?;
                            return Ok(count);
                        }
                    }
                }
                // Pattern didn't match — error like Remove
                return Err(format!(
                    "epoch migration error at {}: `{}` — {}",
                    syntax.span, head_sym, message
                ));
            }
        }
    }

    // Check for Replace match on list forms before the main match.
    // We extract the head symbol and do the lookup before mutating,
    // to satisfy the borrow checker.
    if let SyntaxKind::List(items) = &syntax.kind {
        if let Some(head_sym) = items.first().and_then(|s| s.as_symbol()) {
            if let Some(&(_, arity, template)) = replaces.iter().find(|(s, _, _)| *s == head_sym) {
                if items.len() - 1 == arity {
                    let args: Vec<Syntax> = items[1..].to_vec();
                    let span = syntax.span.clone();
                    let replacement = instantiate_template(template, &args, &span)?;
                    syntax.kind = replacement.kind;
                    count += 1;
                    // Recurse into the replacement so renames and nested
                    // replacements still apply.
                    count += rewrite_node(syntax, renames, removals, replaces, unwraps)?;
                    return Ok(count);
                }
            }
        }
    }

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
                count += rewrite_node(item, renames, removals, replaces, unwraps)?;
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
            count += rewrite_node(inner, renames, removals, replaces, unwraps)?;
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

/// Parse a template string and substitute `$N` placeholders with argument nodes.
fn instantiate_template(template: &str, args: &[Syntax], span: &Span) -> Result<Syntax, String> {
    // Build the instantiated source by replacing $N with Display output
    // of each argument. Iterate in reverse so $10 is replaced before $1.
    let mut source = template.to_string();
    for (i, arg) in args.iter().enumerate().rev() {
        let placeholder = format!("${}", i + 1);
        source = source.replace(&placeholder, &arg.to_string());
    }

    let mut parsed = crate::reader::read_syntax(&source, "<epoch-template>")
        .map_err(|e| format!("epoch migration template error: {}", e))?;

    set_span_recursive(&mut parsed, span);
    Ok(parsed)
}

/// Propagate a span onto all nodes in a tree so error messages point
/// to the original source location.
fn set_span_recursive(syntax: &mut Syntax, span: &Span) {
    syntax.span = span.clone();
    match &mut syntax.kind {
        SyntaxKind::List(items)
        | SyntaxKind::Array(items)
        | SyntaxKind::ArrayMut(items)
        | SyntaxKind::Struct(items)
        | SyntaxKind::StructMut(items)
        | SyntaxKind::Set(items)
        | SyntaxKind::SetMut(items) => {
            for item in items.iter_mut() {
                set_span_recursive(item, span);
            }
        }
        SyntaxKind::Quote(inner)
        | SyntaxKind::Quasiquote(inner)
        | SyntaxKind::Unquote(inner)
        | SyntaxKind::UnquoteSplicing(inner)
        | SyntaxKind::Splice(inner) => {
            set_span_recursive(inner, span);
        }
        _ => {}
    }
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
        let replaces = vec![];

        let mut form = sym("foo");
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert_eq!(count, 1);
        assert_eq!(form.as_symbol(), Some("bar"));
    }

    #[test]
    fn test_rename_in_list() {
        let renames: HashMap<&str, &str> = [("old", "new")].into_iter().collect();
        let removals = HashMap::new();
        let replaces = vec![];

        let mut form = list(vec![sym("old"), int(1), sym("old")]);
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

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
        let replaces = vec![];

        let mut form = Syntax::new(SyntaxKind::Quote(Box::new(sym("foo"))), Span::synthetic());
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert_eq!(count, 0);
        if let SyntaxKind::Quote(inner) = &form.kind {
            assert_eq!(inner.as_symbol(), Some("foo"));
        }
    }

    #[test]
    fn test_rewrite_inside_quasiquote() {
        let renames: HashMap<&str, &str> = [("foo", "bar")].into_iter().collect();
        let removals = HashMap::new();
        let replaces = vec![];

        let mut form = Syntax::new(
            SyntaxKind::Quasiquote(Box::new(sym("foo"))),
            Span::synthetic(),
        );
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

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
        let replaces = vec![];

        let mut form = sym("gone");
        let result = rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new());

        assert!(result.is_err());
        assert!(result.unwrap_err().contains("has been removed"));
    }

    #[test]
    fn test_no_changes_no_rules() {
        let renames = HashMap::new();
        let removals = HashMap::new();
        let replaces = vec![];

        let mut form = list(vec![sym("foo"), int(1)]);
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert_eq!(count, 0);
    }

    #[test]
    fn test_migrate_empty_range() {
        let mut forms = vec![list(vec![sym("foo"), int(1)])];
        let count = migrate(&mut forms, 0, 0).unwrap();
        assert_eq!(count, 0);
    }

    // --- Replace rule tests ---

    #[test]
    fn test_replace_basic() {
        // (assert-eq X Y msg) → (assert (= X Y) msg)
        let renames = HashMap::new();
        let removals = HashMap::new();
        let replaces = vec![("assert-eq", 3usize, "(assert (= $1 $2) $3)")];

        let mut form = list(vec![
            sym("assert-eq"),
            int(1),
            int(2),
            Syntax::new(SyntaxKind::String("msg".to_string()), Span::synthetic()),
        ]);
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert!(count >= 1);
        // Result should be (assert (= 1 2) "msg")
        if let SyntaxKind::List(items) = &form.kind {
            assert_eq!(items[0].as_symbol(), Some("assert"));
            if let SyntaxKind::List(inner) = &items[1].kind {
                assert_eq!(inner[0].as_symbol(), Some("="));
            } else {
                panic!("expected inner list (= ...)");
            }
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_replace_with_complex_args() {
        // (assert-eq (+ 1 2) (- 5 2) "arith") → (assert (= (+ 1 2) (- 5 2)) "arith")
        let renames = HashMap::new();
        let removals = HashMap::new();
        let replaces = vec![("assert-eq", 3usize, "(assert (= $1 $2) $3)")];

        let mut form = list(vec![
            sym("assert-eq"),
            list(vec![sym("+"), int(1), int(2)]),
            list(vec![sym("-"), int(5), int(2)]),
            Syntax::new(SyntaxKind::String("arith".to_string()), Span::synthetic()),
        ]);
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert!(count >= 1);
        if let SyntaxKind::List(items) = &form.kind {
            assert_eq!(items[0].as_symbol(), Some("assert"));
            if let SyntaxKind::List(eq_form) = &items[1].kind {
                assert_eq!(eq_form[0].as_symbol(), Some("="));
                // First arg should be (+ 1 2)
                if let SyntaxKind::List(plus) = &eq_form[1].kind {
                    assert_eq!(plus[0].as_symbol(), Some("+"));
                } else {
                    panic!("expected (+ 1 2)");
                }
            } else {
                panic!("expected (= ...)");
            }
        } else {
            panic!("expected list");
        }
    }

    #[test]
    fn test_replace_arity_mismatch_passthrough() {
        // (assert-eq X Y) with arity 2 should NOT match a rule expecting arity 3
        let renames = HashMap::new();
        let removals = HashMap::new();
        let replaces = vec![("assert-eq", 3usize, "(assert (= $1 $2) $3)")];

        let mut form = list(vec![sym("assert-eq"), int(1), int(2)]);
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert_eq!(count, 0);
        if let SyntaxKind::List(items) = &form.kind {
            assert_eq!(items[0].as_symbol(), Some("assert-eq"));
        }
    }

    #[test]
    fn test_replace_and_rename_together() {
        // Replace (old-fn X Y) → (new-fn (+ $1 $2))
        // Also rename "old-sym" → "new-sym"
        // Input: (old-fn old-sym 2)
        // Expected: (new-fn (+ new-sym 2))
        let renames: HashMap<&str, &str> = [("old-sym", "new-sym")].into_iter().collect();
        let removals = HashMap::new();
        let replaces = vec![("old-fn", 2usize, "(new-fn (+ $1 $2))")];

        let mut form = list(vec![sym("old-fn"), sym("old-sym"), int(2)]);
        let count =
            rewrite_node(&mut form, &renames, &removals, &replaces, &HashMap::new()).unwrap();

        assert!(count >= 2); // at least 1 replace + 1 rename
        if let SyntaxKind::List(items) = &form.kind {
            assert_eq!(items[0].as_symbol(), Some("new-fn"));
            if let SyntaxKind::List(inner) = &items[1].kind {
                assert_eq!(inner[0].as_symbol(), Some("+"));
                // old-sym should have been renamed to new-sym after replacement
                assert_eq!(inner[1].as_symbol(), Some("new-sym"));
            } else {
                panic!("expected inner list");
            }
        }
    }
}
