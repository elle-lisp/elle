//! Syntax tree migration transformer.
//!
//! Walks a syntax tree and applies epoch migration rules in a single pass.
//! Renames are O(1) per symbol node (hash lookup). Replacements match call
//! forms by head symbol and arity, then restructure using a template. The
//! tree is walked once regardless of how many epochs are being crossed.

use crate::syntax::{Span, Syntax, SyntaxKind};
use std::collections::HashMap;

use super::rules::{
    collapsed_renames, flatten_clause_rules_in_range, flatten_rules_in_range, removals_in_range,
    replace_rules_in_range, unwrap_rules_in_range,
};

/// Every rule the walk applies, collected for one epoch range.
///
/// The six tables travel together through the whole recursion, and three of
/// them are `HashMap<&str, &str>` — passed positionally they could be swapped
/// at a call site with no compile error, and a rename table read as removals
/// would reject every migrated symbol. Naming the fields removes that. Build
/// one with [`Rules::for_range`]; the tests build partial sets field by field
/// from [`Rules::none`].
#[derive(Default)]
pub(super) struct Rules<'a> {
    pub renames: HashMap<&'a str, &'a str>,
    pub removals: HashMap<&'a str, &'a str>,
    pub replaces: Vec<(&'a str, usize, &'a str)>,
    pub unwraps: HashMap<&'a str, &'a str>,
    pub flattens: Vec<&'a str>,
    pub flatten_clauses: Vec<(&'a str, usize)>,
}

impl Rules<'static> {
    /// The rules that apply when crossing from `from_epoch` to `to_epoch`.
    pub(super) fn for_range(from_epoch: u64, to_epoch: u64) -> Self {
        Rules {
            renames: collapsed_renames(from_epoch, to_epoch),
            removals: removals_in_range(from_epoch, to_epoch),
            replaces: replace_rules_in_range(from_epoch, to_epoch),
            unwraps: unwrap_rules_in_range(from_epoch, to_epoch),
            flattens: flatten_rules_in_range(from_epoch, to_epoch),
            flatten_clauses: flatten_clause_rules_in_range(from_epoch, to_epoch),
        }
    }
}

impl Rules<'_> {
    /// No rules at all — the walk rewrites nothing.
    ///
    /// `doc` joins `test` in the gate because a doc build does not set `test`,
    /// so rustdoc would not see the item the `Rules` docs link to. CI runs
    /// rustdoc with `-D warnings`, which turns that dangling link into an error.
    #[cfg(any(test, doc))]
    pub(super) fn none() -> Self {
        Rules::default()
    }

    /// True when no rule of any kind applies, so the walk can be skipped.
    fn is_empty(&self) -> bool {
        self.renames.is_empty()
            && self.removals.is_empty()
            && self.replaces.is_empty()
            && self.unwraps.is_empty()
            && self.flattens.is_empty()
            && self.flatten_clauses.is_empty()
    }
}

/// Migrate syntax forms from `from_epoch` to `to_epoch`.
///
/// Applies all renames and replacements in one pass. Returns the number
/// of nodes rewritten. Returns `Err` if a removed form is encountered.
pub fn migrate(forms: &mut [Syntax], from_epoch: u64, to_epoch: u64) -> Result<usize, String> {
    let rules = Rules::for_range(from_epoch, to_epoch);
    if rules.is_empty() {
        return Ok(0);
    }

    let mut count = 0;
    for form in forms.iter_mut() {
        count += rewrite_node(form, &rules)?;
    }
    Ok(count)
}

/// Recursively rewrite a single syntax node.
pub(super) fn rewrite_node(syntax: &mut Syntax, rules: &Rules<'_>) -> Result<usize, String> {
    let Rules {
        renames,
        removals,
        replaces,
        unwraps,
        flattens,
        flatten_clauses,
    } = rules;
    let mut count = 0;

    // Check for FlattenClauses match: (cond (test body) ...) → (cond test body ...)
    // and (match val (pat body) ...) → (match val pat body ...)
    if !flatten_clauses.is_empty() {
        if let SyntaxKind::List(items) = &mut syntax.kind {
            if let Some(head_sym) = items.first().and_then(|s| s.as_symbol()).map(String::from) {
                if let Some(&(_, skip)) = flatten_clauses
                    .iter()
                    .find(|(s, _)| *s == head_sym.as_str())
                {
                    let clause_start = 1 + skip; // skip head symbol + skip args
                    if items.len() > clause_start {
                        let mut new_items: Vec<Syntax> = items[..clause_start].to_vec();
                        let mut changed = false;
                        for clause in &items[clause_start..] {
                            if let Some(parts) = clause.as_list_or_tuple() {
                                if parts.is_empty() {
                                    continue;
                                }
                                // (else body) in cond → just the body as trailing default
                                if parts[0].as_symbol() == Some("else") {
                                    if parts.len() == 2 {
                                        new_items.push(parts[1].clone());
                                    } else if parts.len() > 2 {
                                        let span = clause.span.clone();
                                        let mut begin_items = vec![Syntax::new(
                                            SyntaxKind::Symbol("begin".to_string()),
                                            span,
                                        )];
                                        begin_items.extend(parts[1..].to_vec());
                                        new_items.push(Syntax::new(
                                            SyntaxKind::List(begin_items),
                                            clause.span.clone(),
                                        ));
                                    }
                                    changed = true;
                                    continue;
                                }
                                // 2-element clause: (test body) → test body
                                if parts.len() == 2 {
                                    new_items.push(parts[0].clone());
                                    new_items.push(parts[1].clone());
                                    changed = true;
                                } else if parts.len() > 2 {
                                    // Multi-element: (pat body1 body2) → pat (begin body1 body2)
                                    // But check for 'when' guard in match:
                                    // (pat when guard body) → pat when guard body
                                    new_items.push(parts[0].clone());
                                    if parts.len() >= 3 && parts[1].as_symbol() == Some("when") {
                                        // Guard: splice all remaining elements
                                        for part in &parts[1..] {
                                            new_items.push(part.clone());
                                        }
                                    } else {
                                        // Multi-body: wrap in begin
                                        let span = clause.span.clone();
                                        let mut begin_items = vec![Syntax::new(
                                            SyntaxKind::Symbol("begin".to_string()),
                                            span,
                                        )];
                                        begin_items.extend(parts[1..].to_vec());
                                        new_items.push(Syntax::new(
                                            SyntaxKind::List(begin_items),
                                            clause.span.clone(),
                                        ));
                                    }
                                    changed = true;
                                } else {
                                    // Single-element clause — shouldn't happen, pass through
                                    new_items.push(clause.clone());
                                }
                            } else {
                                // Not a parenthesized clause — already flat, pass through
                                new_items.push(clause.clone());
                            }
                        }
                        if changed {
                            *items = new_items;
                            count += 1;
                        }
                    }
                }
            }
        }
    }

    // Check for FlattenBindings match: (let|letrec [[p1 v1] [p2 v2] ...] body...)
    // Detect nested-pair format and flatten to (let|letrec [p1 v1 p2 v2 ...] body...)
    if !flattens.is_empty() {
        if let SyntaxKind::List(items) = &mut syntax.kind {
            if let Some(head_sym) = items.first().and_then(|s| s.as_symbol()).map(String::from) {
                if flattens.contains(&head_sym.as_str()) && items.len() >= 2 {
                    if let SyntaxKind::Array(bindings) | SyntaxKind::List(bindings) =
                        &mut items[1].kind
                    {
                        // Detect nested-pair format: every child is a 2-element list/array
                        let all_pairs = !bindings.is_empty()
                            && bindings.iter().all(|b| {
                                matches!(&b.kind, SyntaxKind::List(v) | SyntaxKind::Array(v) if v.len() == 2)
                            });
                        if all_pairs {
                            // Flatten: splice each pair's contents into parent
                            let mut flat = Vec::with_capacity(bindings.len() * 2);
                            for binding in bindings.drain(..) {
                                match binding.kind {
                                    SyntaxKind::List(mut pair) | SyntaxKind::Array(mut pair) => {
                                        flat.append(&mut pair);
                                    }
                                    _ => unreachable!(),
                                }
                            }
                            *bindings = flat;
                            count += 1;
                        }
                    }
                }
            }
        }
    }

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
                            count += rewrite_node(syntax, rules)?;
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
                    count += rewrite_node(syntax, rules)?;
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
        | SyntaxKind::SetMut(items)
        | SyntaxKind::Bytes(items)
        | SyntaxKind::BytesMut(items) => {
            for item in items.iter_mut() {
                count += rewrite_node(item, rules)?;
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
            count += rewrite_node(inner, rules)?;
        }

        // Atoms — nothing to rewrite.
        SyntaxKind::Nil
        | SyntaxKind::Bool(_)
        | SyntaxKind::Int(_)
        | SyntaxKind::Float(_)
        | SyntaxKind::Keyword(_)
        | SyntaxKind::String(_)
        | SyntaxKind::StringMut(_)
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
        | SyntaxKind::SetMut(items)
        | SyntaxKind::Bytes(items)
        | SyntaxKind::BytesMut(items) => {
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
mod tests;
