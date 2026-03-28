//! CLI entry point for `elle rewrite`.

use super::edit::{apply_edits, Edit};
use super::engine::collect_edits;
use super::rule::{RenameSymbol, RewriteRule};
use crate::epoch::detect_epoch_in_source;
use crate::epoch::rules::{
    collapsed_renames, removals_in_range, replace_rules_in_range, unwrap_rules_in_range,
    CURRENT_EPOCH,
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

    // Merge all structural edits (unwrap + replace), filtering out
    // rename edits that fall within their spans.
    let structural_edits: Vec<Edit> = replace_edits.into_iter().chain(unwrap_edits).collect();
    if !structural_edits.is_empty() {
        edits.retain(|edit| {
            !structural_edits.iter().any(|re| {
                edit.byte_offset >= re.byte_offset
                    && edit.byte_offset + edit.byte_len <= re.byte_offset + re.byte_len
            })
        });
        edits.extend(structural_edits);
    }

    // Replace old (elle/epoch N) with current epoch, or add it if absent
    if let Some(info) = &epoch_info {
        edits.push(Edit {
            byte_offset: info.byte_start,
            byte_len: info.byte_end - info.byte_start,
            replacement: format!("(elle/epoch {})", CURRENT_EPOCH),
        });
    } else if !edits.is_empty() {
        // File had no epoch tag but needed rewrites — prepend the current epoch
        edits.push(Edit {
            byte_offset: 0,
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
