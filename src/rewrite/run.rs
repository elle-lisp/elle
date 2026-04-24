//! CLI entry point for `elle rewrite`.

use super::edit::{apply_edits, Edit};
use super::engine::collect_edits;
use super::rule::{RenameSymbol, RewriteRule};
use crate::epoch::detect_epoch_in_source;
use crate::epoch::rules::{
    collapsed_renames, flatten_clause_rules_in_range, flatten_rules_in_range, removals_in_range,
    replace_rules_in_range, unwrap_rules_in_range, CURRENT_EPOCH,
};
use crate::reader::{Lexer, Token};
use std::collections::HashMap;

/// Run the rewrite tool. Returns exit code.
/// Exit codes: 0 = success (or no changes in --check mode), 1 = changes needed (--check) or error.
pub fn run(args: &[String]) -> i32 {
    let mut check = false;
    let mut dry_run = false;
    let mut list_rules = false;
    let mut files = Vec::new();

    let mut i = 0;
    while i < args.len() {
        match args[i].as_str() {
            "--check" => check = true,
            "--dry-run" => dry_run = true,
            "--list-rules" => list_rules = true,
            "--help" | "-h" => {
                print_help();
                return 0;
            }
            other => {
                if !other.starts_with('-') {
                    files.push(other.to_string());
                } else {
                    eprintln!("Unknown option: {}", other);
                    return 1;
                }
            }
        }
        i += 1;
    }

    if list_rules {
        println!("Epoch migration rules (current epoch: {}):", CURRENT_EPOCH);
        if CURRENT_EPOCH == 0 {
            println!("  (none — epoch 0)");
        } else {
            let renames = collapsed_renames(0, CURRENT_EPOCH);
            for (old, new) in &renames {
                println!("  rename: {} → {}", old, new);
            }
            let removals = removals_in_range(0, CURRENT_EPOCH);
            for (sym, msg) in &removals {
                println!("  remove: {} ({})", sym, msg);
            }
            let replaces = replace_rules_in_range(0, CURRENT_EPOCH);
            for (sym, arity, template) in &replaces {
                println!("  replace: {} (arity {}) → {}", sym, arity, template);
            }
            let unwraps = unwrap_rules_in_range(0, CURRENT_EPOCH);
            for (sym, msg) in &unwraps {
                println!("  unwrap:  {} ({})", sym, msg);
            }
            let flattens = flatten_rules_in_range(0, CURRENT_EPOCH);
            if !flattens.is_empty() {
                println!(
                    "  flatten: {} (nested-pair → flat bindings)",
                    flattens.join(", ")
                );
            }
            let flatten_clauses = flatten_clause_rules_in_range(0, CURRENT_EPOCH);
            if !flatten_clauses.is_empty() {
                let names: Vec<&str> = flatten_clauses.iter().map(|(s, _)| *s).collect();
                println!(
                    "  flatten-clauses: {} (parenthesized → flat pairs)",
                    names.join(", ")
                );
            }
        }
        return 0;
    }

    if files.is_empty() {
        eprintln!("Error: no files specified");
        print_help();
        return 1;
    }

    let mut any_changes = false;
    let mut had_errors = false;

    for file_path in &files {
        let source = match std::fs::read_to_string(file_path) {
            Ok(s) => s,
            Err(e) => {
                eprintln!("Error reading {}: {}", file_path, e);
                had_errors = true;
                continue;
            }
        };

        match rewrite_file(&source, file_path) {
            Ok(None) => {} // no changes
            Ok(Some((result, edit_count))) => {
                any_changes = true;
                if check {
                    println!("{}: {} edit(s) needed", file_path, edit_count);
                } else if dry_run {
                    println!("{}: {} edit(s) would be applied", file_path, edit_count);
                } else {
                    if let Err(e) = std::fs::write(file_path, &result) {
                        eprintln!("Error writing {}: {}", file_path, e);
                        had_errors = true;
                        continue;
                    }
                    println!("{}: {} edit(s) applied", file_path, edit_count);
                }
            }
            Err(e) => {
                eprintln!("Error rewriting {}: {}", file_path, e);
                had_errors = true;
            }
        }
    }

    if had_errors {
        return 1;
    }

    if check && any_changes {
        return 1;
    }

    0
}

/// Rewrite a single file's source. Returns `Ok(None)` if no changes needed,
/// `Ok(Some((new_source, edit_count)))` if changes were made.
fn rewrite_file(source: &str, file_path: &str) -> Result<Option<(String, usize)>, String> {
    // Detect epoch
    let epoch_info = detect_epoch_in_source(source)?;

    let file_epoch = epoch_info.as_ref().map(|info| info.epoch);

    // Check for removed symbols before doing any rewrites
    if let Some(epoch) = file_epoch {
        let removals = removals_in_range(epoch, CURRENT_EPOCH);
        if !removals.is_empty() {
            check_removals(source, &removals, file_path)?;
        }
    }

    // Collect unwrap edits: (symbol (fn [] body...)) → body...
    let unwrap_edits = if let Some(epoch) = file_epoch {
        let unwraps = unwrap_rules_in_range(epoch, CURRENT_EPOCH);
        if unwraps.is_empty() {
            Vec::new()
        } else {
            collect_unwrap_edits(source, &unwraps, file_path)?
        }
    } else {
        Vec::new()
    };

    // Collect flatten edits: [[p1 v1] [p2 v2]] → [p1 v1 p2 v2]
    let flatten_edits = if let Some(epoch) = file_epoch {
        let flattens = flatten_rules_in_range(epoch, CURRENT_EPOCH);
        if flattens.is_empty() {
            Vec::new()
        } else {
            collect_flatten_edits(source, &flattens)?
        }
    } else {
        Vec::new()
    };

    // Collect flatten-clause edits: (cond (test body) ...) → (cond test body ...)
    let flatten_clause_edits = if let Some(epoch) = file_epoch {
        let flatten_clauses = flatten_clause_rules_in_range(epoch, CURRENT_EPOCH);
        if flatten_clauses.is_empty() {
            Vec::new()
        } else {
            collect_flatten_clause_edits(source, &flatten_clauses)?
        }
    } else {
        Vec::new()
    };

    // Normalize paren-delimited binding vectors to brackets:
    // (let (name val) ...) → (let [name val] ...)
    let binding_forms = &["let", "letrec", "let*", "if-let", "when-let", "when-ok"];
    let bracket_edits = collect_bracket_edits(source, binding_forms)?;

    // Collect replace edits (syntax-level, whole-form rewrites)
    let replace_edits = if let Some(epoch) = file_epoch {
        let replaces = replace_rules_in_range(epoch, CURRENT_EPOCH);
        if replaces.is_empty() {
            Vec::new()
        } else {
            collect_replace_edits(source, &replaces)?
        }
    } else {
        Vec::new()
    };

    // Build rename rules for this file's epoch
    let rename_rule = file_epoch.and_then(|epoch| {
        let renames = collapsed_renames(epoch, CURRENT_EPOCH);
        if renames.is_empty() {
            return None;
        }
        let owned: HashMap<String, String> = renames
            .into_iter()
            .map(|(k, v)| (k.to_string(), v.to_string()))
            .collect();
        Some(RenameSymbol::new("epoch-migration", owned))
    });

    // Collect rename edits (token-level)
    let rules: Vec<&dyn RewriteRule> = rename_rule.iter().map(|r| r as &dyn RewriteRule).collect();
    let mut edits = collect_edits(source, &rules)?;

    // Merge all structural edits (unwrap + replace + flatten), filtering out
    // rename edits that fall within their spans.
    let structural_edits: Vec<Edit> = replace_edits
        .into_iter()
        .chain(unwrap_edits)
        .chain(flatten_edits)
        .chain(flatten_clause_edits)
        .chain(bracket_edits)
        .collect();
    if !structural_edits.is_empty() {
        edits.retain(|edit| {
            !structural_edits.iter().any(|re| {
                edit.byte_offset >= re.byte_offset
                    && edit.byte_offset + edit.byte_len <= re.byte_offset + re.byte_len
            })
        });
        edits.extend(structural_edits);
    }

    // Update the epoch tag: replace old tag with current, or add if missing.
    // Files should always carry an epoch tag for forward compatibility.
    let needs_epoch_update = match &epoch_info {
        Some(info) if info.epoch == CURRENT_EPOCH => false, // already current
        _ => true,                                          // old epoch or no epoch tag
    };

    if needs_epoch_update {
        // Remove old epoch tag if present.
        if let Some(info) = &epoch_info {
            let mut end = info.byte_end;
            while end < source.len() && source.as_bytes()[end] == b' ' {
                end += 1;
            }
            if end < source.len() && source.as_bytes()[end] == b'\n' {
                end += 1;
            }
            edits.push(Edit {
                byte_offset: info.byte_start,
                byte_len: end - info.byte_start,
                replacement: String::new(),
            });
        }
        // Insert current epoch as the first form, after the shebang if present.
        let insert_offset = if source.starts_with("#!") {
            source.find('\n').map(|i| i + 1).unwrap_or(source.len())
        } else {
            0
        };
        edits.push(Edit {
            byte_offset: insert_offset,
            byte_len: 0,
            replacement: format!("(elle/epoch {})\n", CURRENT_EPOCH),
        });
    }

    if edits.is_empty() {
        return Ok(None);
    }

    let edit_count = edits.len();
    let result = apply_edits(source, &mut edits)?;
    Ok(Some((result, edit_count)))
}

/// Scan source for removed symbols and return an error listing them.
fn check_removals(
    source: &str,
    removals: &HashMap<&str, &str>,
    file_path: &str,
) -> Result<(), String> {
    use crate::reader::{Lexer, Token};

    let mut lexer = Lexer::new(source);
    let mut errors = Vec::new();

    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(token)) => {
                if let Token::Symbol(name) = &token.token {
                    if let Some(msg) = removals.get(*name) {
                        errors.push(format!("  `{}` has been removed — {}", name, msg));
                    }
                }
            }
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    if errors.is_empty() {
        Ok(())
    } else {
        Err(format!(
            "{}: removed symbols found:\n{}",
            file_path,
            errors.join("\n")
        ))
    }
}

/// Lex source and collect edits for forms matching unwrap rules.
/// Matches `(symbol (fn [] body...))` or `(symbol (fn () body...))` and
/// replaces the entire form with just the body.
fn collect_unwrap_edits(
    source: &str,
    unwraps: &HashMap<&str, &str>,
    file_path: &str,
) -> Result<Vec<Edit>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens: Vec<(Token<'_>, usize, usize)> = Vec::new();
    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(t)) => tokens.push((t.token, t.byte_offset, t.len)),
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(edit) = try_match_unwrap(source, &tokens, i, unwraps) {
            i = skip_balanced_form(&tokens, i);
            edits.push(edit);
        } else {
            // Check for non-unwrappable uses (ev/run with wrong pattern)
            if let Token::Symbol(name) = &tokens[i].0 {
                if let Some(msg) = unwraps.get(*name) {
                    // Check if this is in head position of a list
                    if i > 0 && matches!(tokens[i - 1].0, Token::LeftParen) {
                        return Err(format!(
                            "{}: `{}` cannot be automatically unwrapped — {}",
                            file_path, name, msg
                        ));
                    }
                }
            }
            i += 1;
        }
    }
    Ok(edits)
}

/// Try to match an unwrap rule: `(symbol (fn [] body...))` → `body...`
fn try_match_unwrap<'a>(
    source: &str,
    tokens: &[(Token<'a>, usize, usize)],
    i: usize,
    unwraps: &HashMap<&str, &str>,
) -> Option<Edit> {
    // Must be `(` symbol `(` fn `[]` or `()` ...body... `)` `)`
    if !matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
        return None;
    }
    let head_sym = match tokens.get(i + 1) {
        Some((Token::Symbol(s), _, _)) => *s,
        _ => return None,
    };
    if !unwraps.contains_key(head_sym) {
        return None;
    }
    // Next must be `(` fn
    if !matches!(tokens.get(i + 2), Some((Token::LeftParen, _, _))) {
        return None;
    }
    if !matches!(tokens.get(i + 3), Some((Token::Symbol(s), _, _)) if *s == "fn") {
        return None;
    }
    // Next must be `[]` or `()`
    let params_start = i + 4;
    let params_end = match tokens.get(params_start) {
        Some((Token::LeftBracket, _, _)) => {
            // Check for empty brackets: [ ]
            if matches!(
                tokens.get(params_start + 1),
                Some((Token::RightBracket, _, _))
            ) {
                params_start + 2
            } else {
                return None; // non-empty params
            }
        }
        Some((Token::LeftParen, _, _)) => {
            // Check for empty parens: ( )
            if matches!(
                tokens.get(params_start + 1),
                Some((Token::RightParen, _, _))
            ) {
                params_start + 2
            } else {
                return None; // non-empty params
            }
        }
        _ => return None,
    };

    // Body starts at params_end, ends before the inner `)` of `(fn [] body...)`
    // then the outer `)` of `(ev/run ...)`
    // Find the body text: from first body token to before inner `)`
    let body_start_byte = tokens.get(params_end).map(|t| t.1)?;

    // Find the matching `)` for the `(fn` — walk balanced from i+2
    let inner_close = skip_balanced_form(tokens, i + 2);
    if inner_close == 0 {
        return None;
    }
    let inner_close_idx = inner_close - 1; // index of the `)` token

    // Body ends before this `)`
    let body_end_byte = tokens.get(inner_close_idx).map(|t| t.1)?;

    // The outer form spans from `(` at i to `)` after the inner close
    let outer_close = skip_balanced_form(tokens, i);
    let form_start = tokens[i].1;
    let form_end = tokens.get(outer_close - 1).map(|t| t.1 + t.2)?;

    let body_text = source[body_start_byte..body_end_byte].trim();

    Some(Edit {
        byte_offset: form_start,
        byte_len: form_end - form_start,
        replacement: body_text.to_string(),
    })
}

/// Lex source and collect edits for forms matching replace rules.
/// Works at the token level using byte offsets from the lexer.
fn collect_replace_edits(
    source: &str,
    replaces: &[(&str, usize, &str)],
) -> Result<Vec<Edit>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens: Vec<(Token<'_>, usize, usize)> = Vec::new(); // (token, byte_offset, len)
    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(t)) => tokens.push((t.token, t.byte_offset, t.len)),
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        if let Some(edit) = try_match_replace(source, &tokens, i, replaces) {
            // Skip past the matched form
            i = skip_balanced_form(&tokens, i);
            edits.push(edit);
        } else {
            i += 1;
        }
    }
    Ok(edits)
}

/// Try to match a replace rule at token position `i`.
/// Expects `tokens[i]` to be `LeftParen` followed by a matching symbol.
fn try_match_replace<'a>(
    source: &str,
    tokens: &[(Token<'a>, usize, usize)],
    i: usize,
    replaces: &[(&str, usize, &str)],
) -> Option<Edit> {
    // Must start with LeftParen
    if !matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
        return None;
    }
    // Next token must be a symbol matching a replace rule
    let head_sym = match tokens.get(i + 1) {
        Some((Token::Symbol(s), _, _)) => *s,
        _ => return None,
    };
    let (_, arity, template) = replaces.iter().find(|(s, _, _)| *s == head_sym)?;

    // Collect argument byte ranges by walking balanced tokens
    let mut args: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte) per arg
    let mut pos = i + 2; // skip LeftParen and head symbol
    while pos < tokens.len() {
        match &tokens[pos].0 {
            Token::RightParen => break,
            _ => {
                let arg_start = tokens[pos].1;
                let arg_end_pos = skip_one_form(tokens, pos);
                if arg_end_pos == 0 || arg_end_pos > tokens.len() {
                    return None; // malformed
                }
                let last = arg_end_pos - 1;
                let arg_end = tokens[last].1 + tokens[last].2;
                args.push((arg_start, arg_end));
                pos = arg_end_pos;
            }
        }
    }

    if pos >= tokens.len() || args.len() != *arity {
        return None;
    }

    // Form spans from LeftParen to RightParen (inclusive)
    let form_start = tokens[i].1;
    let form_end = tokens[pos].1 + tokens[pos].2; // byte after )

    // Build replacement by interpolating source text of args into template
    let mut result = template.to_string();
    for (j, (start, end)) in args.iter().enumerate().rev() {
        let placeholder = format!("${}", j + 1);
        result = result.replace(&placeholder, &source[*start..*end]);
    }

    Some(Edit {
        byte_offset: form_start,
        byte_len: form_end - form_start,
        replacement: result,
    })
}

/// Skip past one balanced form starting at `pos`. Returns the index after the form.
fn skip_one_form(tokens: &[(Token<'_>, usize, usize)], pos: usize) -> usize {
    match &tokens[pos].0 {
        Token::LeftParen | Token::LeftBracket | Token::LeftBrace => skip_balanced_form(tokens, pos),
        // |...| set literal — scan to matching |
        Token::Pipe => skip_pipe_form(tokens, pos),
        // Prefix tokens: skip the prefix then the following form
        Token::Quote
        | Token::Quasiquote
        | Token::Unquote
        | Token::UnquoteSplicing
        | Token::Splice => skip_one_form(tokens, pos + 1),
        // @[...], @{...} — prefix then balanced form
        Token::ListSugar => skip_one_form(tokens, pos + 1),
        // @|...| — scan for closing |
        Token::AtPipe => skip_pipe_form(tokens, pos),
        _ => pos + 1, // atom: single token
    }
}

/// Skip a balanced delimited form (list/array/struct) starting at `pos`.
/// Returns the index after the closing delimiter.
fn skip_balanced_form(tokens: &[(Token<'_>, usize, usize)], start: usize) -> usize {
    let mut depth = 0i32;
    let mut pos = start;
    while pos < tokens.len() {
        match &tokens[pos].0 {
            Token::LeftParen | Token::LeftBracket | Token::LeftBrace => depth += 1,
            Token::RightParen | Token::RightBracket | Token::RightBrace => {
                depth -= 1;
                if depth == 0 {
                    return pos + 1;
                }
            }
            _ => {}
        }
        pos += 1;
    }
    pos
}

/// Skip a `|...|` set literal. Scan for the matching closing `|`.
fn skip_pipe_form(tokens: &[(Token<'_>, usize, usize)], start: usize) -> usize {
    let mut pos = start + 1; // skip opening |
    while pos < tokens.len() {
        if matches!(tokens[pos].0, Token::Pipe) {
            return pos + 1;
        }
        pos = skip_one_form(tokens, pos);
    }
    pos
}

/// Lex source and collect edits that flatten nested-pair binding vectors.
/// Matches `( let|letrec [ [p1 v1] [p2 v2] ... ] body... )` and deletes
/// the inner `[`/`]` (or `(`/`)`) delimiters, leaving the contents flat.
fn collect_flatten_edits(source: &str, flatten_syms: &[&str]) -> Result<Vec<Edit>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens: Vec<(Token<'_>, usize, usize)> = Vec::new();
    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(t)) => tokens.push((t.token, t.byte_offset, t.len)),
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for `(` symbol `[`  where symbol is in flatten_syms
        if matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
            if let Some((Token::Symbol(s), _, _)) = tokens.get(i + 1) {
                if flatten_syms.contains(s) {
                    if let Some(new_edits) = try_match_flatten(source, &tokens, i) {
                        edits.extend(new_edits);
                        // Don't skip the whole form — advance past `(` and symbol
                        // so nested let/letrec forms in the body are still visited.
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    Ok(edits)
}

/// Try to flatten the bindings vector of a let/letrec form at token `i`.
/// Returns edits that delete the inner pair delimiters if the form matches
/// the nested-pair pattern.
fn try_match_flatten(
    _source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    i: usize,
) -> Option<Vec<Edit>> {
    // tokens[i] = `(`, tokens[i+1] = let/letrec, tokens[i+2] should be `[` or `(`
    let bindings_open = i + 2;
    if bindings_open >= tokens.len() {
        return None;
    }
    let open_token = &tokens[bindings_open].0;
    if !matches!(open_token, Token::LeftBracket | Token::LeftParen) {
        return None;
    }

    // Find the matching close of the bindings container
    let bindings_close = skip_balanced_form(tokens, bindings_open);
    if bindings_close == 0 {
        return None;
    }
    let close_idx = bindings_close - 1; // index of the `]` or `)` token

    // Walk direct children of the bindings container.
    // Each child must be a 2-element list/array (the nested-pair format).
    // If any child is an atom, it's already flat — skip.
    let mut pairs: Vec<(usize, usize)> = Vec::new(); // (open_idx, close_idx) for each inner pair
    let mut pos = bindings_open + 1; // skip the opening `[` of bindings
    while pos < close_idx {
        match &tokens[pos].0 {
            Token::LeftBracket | Token::LeftParen => {
                let pair_open = pos;
                let pair_close_next = skip_balanced_form(tokens, pos);
                if pair_close_next == 0 {
                    return None;
                }
                let pair_close = pair_close_next - 1;

                // Count the children of this inner form to verify it has exactly 2
                let mut child_count = 0;
                let mut child_pos = pair_open + 1;
                while child_pos < pair_close {
                    child_count += 1;
                    child_pos = skip_one_form(tokens, child_pos);
                }

                if child_count != 2 {
                    // Not a 2-element pair — this might be a destructuring pattern
                    // in an already-flat binding. Skip this form entirely.
                    return None;
                }

                pairs.push((pair_open, pair_close));
                pos = pair_close_next;
            }
            _ => {
                // Atom found at top level of bindings — already flat
                return None;
            }
        }
    }

    if pairs.is_empty() {
        return None;
    }

    // Generate edits: for each inner pair, delete the opening and closing delimiters.
    // We need to handle whitespace carefully — consume trailing whitespace after the
    // opening delimiter and leading whitespace before the closing delimiter.
    let mut edits = Vec::new();
    for &(open_idx, close_idx) in &pairs {
        let open_byte = tokens[open_idx].1;
        // Delete the opening delimiter. Also consume any whitespace between it and
        // the first child form.
        let next_byte = tokens[open_idx + 1].1;
        edits.push(Edit {
            byte_offset: open_byte,
            byte_len: next_byte - open_byte,
            replacement: String::new(),
        });

        // Delete the closing delimiter. Also consume whitespace before it.
        let close_byte = tokens[close_idx].1;
        let close_len = tokens[close_idx].2;
        let prev_end_idx = close_idx - 1;
        let prev_end = tokens[prev_end_idx].1 + tokens[prev_end_idx].2;
        edits.push(Edit {
            byte_offset: prev_end,
            byte_len: close_byte + close_len - prev_end,
            replacement: String::new(),
        });
    }

    Some(edits)
}

/// Normalize paren-delimited binding vectors to brackets.
/// Matches `(let|letrec|let*|if-let|when-let|when-ok (bindings...) body...)`
/// where the bindings container uses `(...)` and replaces with `[...]`.
fn collect_bracket_edits(source: &str, binding_forms: &[&str]) -> Result<Vec<Edit>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens: Vec<(Token<'_>, usize, usize)> = Vec::new();
    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(t)) => tokens.push((t.token, t.byte_offset, t.len)),
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for `( symbol (` where symbol is a binding form and the
        // bindings container uses parens instead of brackets.
        if matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
            if let Some((Token::Symbol(s), _, _)) = tokens.get(i + 1) {
                if binding_forms.contains(s) {
                    if let Some((Token::LeftParen, open_byte, open_len)) = tokens.get(i + 2) {
                        // Find the matching close paren
                        let close_next = skip_balanced_form(&tokens, i + 2);
                        if close_next > 0 {
                            let close_idx = close_next - 1;
                            let (_, close_byte, close_len) = tokens[close_idx];
                            // Replace `(` with `[` and `)` with `]`
                            edits.push(Edit {
                                byte_offset: *open_byte,
                                byte_len: *open_len,
                                replacement: "[".to_string(),
                            });
                            edits.push(Edit {
                                byte_offset: close_byte,
                                byte_len: close_len,
                                replacement: "]".to_string(),
                            });
                        }
                    }
                }
            }
        }
        i += 1;
    }
    Ok(edits)
}

/// Lex source and collect edits that flatten parenthesized cond/match clauses.
/// Matches `(cond (test body) ...)` or `(match val (pat body) ...)` and
/// removes the inner clause delimiters, wrapping multi-body arms in `(begin ...)`.
fn collect_flatten_clause_edits(
    source: &str,
    flatten_clauses: &[(&str, usize)],
) -> Result<Vec<Edit>, String> {
    let mut lexer = Lexer::new(source);
    let mut tokens: Vec<(Token<'_>, usize, usize)> = Vec::new();
    loop {
        match lexer.next_token_with_loc() {
            Ok(Some(t)) => tokens.push((t.token, t.byte_offset, t.len)),
            Ok(None) => break,
            Err(e) => return Err(e.to_string()),
        }
    }

    let mut edits = Vec::new();
    let mut i = 0;
    while i < tokens.len() {
        // Look for `(` symbol where symbol is in flatten_clauses
        if matches!(tokens.get(i), Some((Token::LeftParen, _, _))) {
            if let Some((Token::Symbol(s), _, _)) = tokens.get(i + 1) {
                if let Some(&(_, skip)) = flatten_clauses.iter().find(|(sym, _)| sym == s) {
                    if let Some(new_edits) = try_match_flatten_clauses(source, &tokens, i, skip) {
                        edits.extend(new_edits);
                        // Don't skip the whole form — advance past head symbol
                        // so nested forms in the body are still visited.
                        i += 2;
                        continue;
                    }
                }
            }
        }
        i += 1;
    }
    Ok(edits)
}

/// Try to flatten the clauses of a cond/match form at token `i`.
/// `skip` is 0 for cond (no args before clauses) or 1 for match (skip value expr).
fn try_match_flatten_clauses(
    source: &str,
    tokens: &[(Token<'_>, usize, usize)],
    i: usize,
    skip: usize,
) -> Option<Vec<Edit>> {
    // tokens[i] = `(`, tokens[i+1] = cond/match
    // Skip past head symbol + `skip` argument forms
    let mut pos = i + 2; // after `(` and symbol
    for _ in 0..skip {
        if pos >= tokens.len() {
            return None;
        }
        pos = skip_one_form(tokens, pos);
    }

    // Find the closing paren of the outer form
    let outer_close = skip_balanced_form(tokens, i);
    if outer_close == 0 {
        return None;
    }
    let outer_close_idx = outer_close - 1;

    // Walk remaining children — each should be a parenthesized clause
    let mut edits = Vec::new();
    let mut any_clause = false;
    while pos < outer_close_idx {
        match &tokens[pos].0 {
            Token::LeftParen | Token::LeftBracket => {
                let clause_open = pos;
                let clause_close_next = skip_balanced_form(tokens, pos);
                if clause_close_next == 0 {
                    return None;
                }
                let clause_close = clause_close_next - 1;

                // Count children and find their positions
                let mut children: Vec<(usize, usize)> = Vec::new(); // (start_byte, end_byte)
                let mut child_pos = clause_open + 1;
                while child_pos < clause_close {
                    let child_start = tokens[child_pos].1;
                    let child_end_pos = skip_one_form(tokens, child_pos);
                    let last = child_end_pos - 1;
                    let child_end = tokens[last].1 + tokens[last].2;
                    children.push((child_start, child_end));
                    child_pos = child_end_pos;
                }

                if children.is_empty() {
                    pos = clause_close_next;
                    continue;
                }

                // Check for (else body) in cond — replace with just body
                let first_text = &source[children[0].0..children[0].1];
                if first_text == "else" && children.len() >= 2 {
                    // Replace entire clause with just the body part(s)
                    let clause_start = tokens[clause_open].1;
                    let clause_end = tokens[clause_close].1 + tokens[clause_close].2;
                    if children.len() == 2 {
                        let body_text = &source[children[1].0..children[1].1];
                        edits.push(Edit {
                            byte_offset: clause_start,
                            byte_len: clause_end - clause_start,
                            replacement: body_text.to_string(),
                        });
                    } else {
                        // Multi-body else: wrap in (begin ...)
                        let body_parts: Vec<&str> =
                            children[1..].iter().map(|(s, e)| &source[*s..*e]).collect();
                        edits.push(Edit {
                            byte_offset: clause_start,
                            byte_len: clause_end - clause_start,
                            replacement: format!("(begin {})", body_parts.join(" ")),
                        });
                    }
                    any_clause = true;
                    pos = clause_close_next;
                    continue;
                }

                // Normal clause: delete delimiters
                if children.len() == 2 {
                    // Simple 2-element clause: just remove the outer parens
                    let open_byte = tokens[clause_open].1;
                    let next_byte = children[0].0;
                    edits.push(Edit {
                        byte_offset: open_byte,
                        byte_len: next_byte - open_byte,
                        replacement: String::new(),
                    });
                    let close_byte = tokens[clause_close].1;
                    let close_len = tokens[clause_close].2;
                    let prev_end = children[children.len() - 1].1;
                    edits.push(Edit {
                        byte_offset: prev_end,
                        byte_len: close_byte + close_len - prev_end,
                        replacement: String::new(),
                    });
                    any_clause = true;
                } else if children.len() >= 3 {
                    // Check for guard pattern: (pat when guard body...)
                    let second_text = &source[children[1].0..children[1].1];
                    if second_text == "when" && children.len() >= 4 {
                        // Guard: just remove outer parens (all elements stay flat)
                        let open_byte = tokens[clause_open].1;
                        let next_byte = children[0].0;
                        edits.push(Edit {
                            byte_offset: open_byte,
                            byte_len: next_byte - open_byte,
                            replacement: String::new(),
                        });
                        let close_byte = tokens[clause_close].1;
                        let close_len = tokens[clause_close].2;
                        let prev_end = children[children.len() - 1].1;
                        edits.push(Edit {
                            byte_offset: prev_end,
                            byte_len: close_byte + close_len - prev_end,
                            replacement: String::new(),
                        });
                    } else {
                        // Multi-body: pattern + (begin body...)
                        let clause_start = tokens[clause_open].1;
                        let clause_end = tokens[clause_close].1 + tokens[clause_close].2;
                        let pattern_text = &source[children[0].0..children[0].1];
                        let body_parts: Vec<&str> =
                            children[1..].iter().map(|(s, e)| &source[*s..*e]).collect();
                        edits.push(Edit {
                            byte_offset: clause_start,
                            byte_len: clause_end - clause_start,
                            replacement: format!(
                                "{} (begin {})",
                                pattern_text,
                                body_parts.join(" ")
                            ),
                        });
                    }
                    any_clause = true;
                } else {
                    // Single-element clause — pass through
                    pos = clause_close_next;
                    continue;
                }

                pos = clause_close_next;
            }
            _ => {
                // Not a parenthesized clause — already flat or atom
                // Don't treat this as needing flattening
                return None;
            }
        }
    }

    if any_clause {
        Some(edits)
    } else {
        None
    }
}

fn print_help() {
    println!("elle rewrite - Source-to-source rewriting tool");
    println!();
    println!("Migrates Elle source files from older epochs to the current epoch");
    println!(
        "(epoch {}). Applies symbol renames and structural replacements,",
        CURRENT_EPOCH
    );
    println!(
        "updates the (elle/epoch N) tag to the current epoch, and reports removed forms that need"
    );
    println!("manual attention.");
    println!();
    println!("Usage: elle rewrite [OPTIONS] <file...>");
    println!();
    println!("Options:");
    println!("  --check        Check if changes are needed (exit 1 if yes)");
    println!("  --dry-run      Show what would change without modifying files");
    println!("  --list-rules   List available rewrite rules");
    println!("  --help, -h     Show this help message");
    println!();
    println!("Examples:");
    println!("  elle rewrite script.lisp");
    println!("  elle rewrite --check src/*.lisp");
    println!("  elle rewrite --dry-run examples/");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_rewrite_preserves_shebang() {
        let source = "#!/usr/bin/env elle\n(elle/epoch 0)\n(assert-true x \"test\")\n";
        let result = rewrite_file(source, "<test>").unwrap();
        assert!(result.is_some(), "expected rewrites to be applied");
        let (new_source, _count) = result.unwrap();
        let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
        let expected_prefix = format!("#!/usr/bin/env elle\n{}", epoch_line);
        assert!(
            new_source.starts_with(&expected_prefix),
            "shebang then epoch tag expected, got: {:?}",
            &new_source[..new_source.len().min(80)]
        );
        // Old epoch tag must not survive
        assert!(
            !new_source.contains("(elle/epoch 0)"),
            "old epoch tag should be removed"
        );
        let epoch_count = new_source.matches("elle/epoch").count();
        assert_eq!(
            epoch_count, 1,
            "should have exactly one epoch tag, got: {:?}",
            new_source
        );
    }

    #[test]
    fn test_rewrite_injects_epoch_first_form() {
        let source = "(elle/epoch 0)\n(assert-true x \"test\")\n";
        let result = rewrite_file(source, "<test>").unwrap();
        assert!(result.is_some());
        let (new_source, _) = result.unwrap();
        let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
        assert!(
            new_source.starts_with(&epoch_line),
            "epoch tag should be the first form, got: {:?}",
            &new_source[..new_source.len().min(80)]
        );
        assert!(
            !new_source.contains("(elle/epoch 0)"),
            "old epoch tag should be removed"
        );
        // Verify no double epoch tags
        let epoch_count = new_source.matches("elle/epoch").count();
        assert_eq!(
            epoch_count, 1,
            "should have exactly one epoch tag, got: {:?}",
            new_source
        );
    }

    #[test]
    fn test_rewrite_no_epoch_tag_injects_one() {
        // File without an epoch tag gets one added (current epoch).
        let source = "(println \"hello\")\n";
        let result = rewrite_file(source, "<test>").unwrap();
        assert!(result.is_some(), "epoch tag should be injected");
        let (new_source, _) = result.unwrap();
        let epoch_line = format!("(elle/epoch {})\n", CURRENT_EPOCH);
        assert!(
            new_source.starts_with(&epoch_line),
            "epoch tag should be first form, got: {:?}",
            &new_source[..new_source.len().min(80)]
        );
    }
}
