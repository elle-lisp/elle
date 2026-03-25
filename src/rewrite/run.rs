//! CLI entry point for `elle rewrite`.

use super::edit::{apply_edits, Edit};
use super::engine::collect_edits;
use super::rule::{RenameSymbol, RewriteRule};
use crate::epoch::detect_epoch_in_source;
use crate::epoch::rules::{
    collapsed_renames, removals_in_range, replace_rules_in_range, CURRENT_EPOCH,
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
            Ok(Some((result, edit_count, details))) => {
                any_changes = true;
                if check {
                    println!("{}: {} edit(s) needed", file_path, edit_count);
                } else if dry_run {
                    println!("{}: {} edit(s) would be applied", file_path, edit_count);
                    for detail in &details {
                        println!("  {}", detail);
                    }
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
/// `Ok(Some((new_source, edit_count, details)))` if changes were made.
fn rewrite_file(
    source: &str,
    file_path: &str,
) -> Result<Option<(String, usize, Vec<String>)>, String> {
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

    // Collect import normalization edits (whole-form rewrites of import-file paths)
    let import_edits = collect_import_edits(source)?;

    // Collect rename edits (token-level)
    let rules: Vec<&dyn RewriteRule> = rename_rule.iter().map(|r| r as &dyn RewriteRule).collect();
    let mut edits = collect_edits(source, &rules)?;

    // Filter out rename edits that fall within replace or import edit spans,
    // then merge the two sets.
    let overlay_edits: Vec<&Edit> = replace_edits.iter().chain(import_edits.iter()).collect();
    if !overlay_edits.is_empty() {
        edits.retain(|edit| {
            !overlay_edits.iter().any(|re| {
                edit.byte_offset >= re.byte_offset
                    && edit.byte_offset + edit.byte_len <= re.byte_offset + re.byte_len
            })
        });
        edits.extend(replace_edits);
        edits.extend(import_edits);
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

    // Build human-readable descriptions of edits (excluding epoch tag updates)
    let details: Vec<String> = edits
        .iter()
        .filter(|e| !e.replacement.starts_with("(elle/epoch"))
        .map(|e| {
            let old = &source[e.byte_offset..e.byte_offset + e.byte_len];
            // Truncate long strings for readability
            let old_display = if old.len() > 60 {
                format!("{}...", &old[..57])
            } else {
                old.to_string()
            };
            format!("{} → {}", old_display, e.replacement)
        })
        .collect();

    let edit_count = edits.len();
    let result = apply_edits(source, &mut edits)?;
    Ok(Some((result, edit_count, details)))
}

/// Result of normalizing an import-file path.
enum ImportNorm {
    /// Elle source module: `(import "name")`
    Source(String),
    /// Native plugin: `(import-native "name")`
    Native(String),
}

/// Extract a bare module name from an import-file path.
///
/// Recognizes three patterns:
/// - `target/{release,debug}/libelle_FOO.{so,dylib}` → Native(FOO)
/// - `lib/PATH.{lisp,elle}` → Source(PATH)
/// - `./lib/PATH.{lisp,elle}` → Source(PATH)
///
/// Returns `None` if the path doesn't match any pattern.
fn normalize_import_path(path: &str) -> Option<ImportNorm> {
    // Pattern 1: target/{release,debug}/libelle_FOO.{so,dylib}
    let stripped = path
        .strip_prefix("target/release/")
        .or_else(|| path.strip_prefix("target/debug/"));
    if let Some(filename) = stripped {
        if let Some(name) = filename
            .strip_prefix("libelle_")
            .and_then(|s| s.strip_suffix(".so").or_else(|| s.strip_suffix(".dylib")))
        {
            return Some(ImportNorm::Native(name.to_string()));
        }
    }

    // Pattern 2/3: [./]lib/PATH.{lisp,elle}
    let stripped = path
        .strip_prefix("./lib/")
        .or_else(|| path.strip_prefix("lib/"));
    if let Some(rest) = stripped {
        if let Some(name) = rest
            .strip_suffix(".lisp")
            .or_else(|| rest.strip_suffix(".elle"))
        {
            return Some(ImportNorm::Source(name.to_string()));
        }
    }

    None
}

/// Scan source for `(import-file STRING)` calls and produce edits that
/// rewrite them to `(import "bare-name")` when the path matches a known
/// pattern. Non-matching paths are left for the rename rule to handle.
fn collect_import_edits(source: &str) -> Result<Vec<Edit>, String> {
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
        // Match: LeftParen, Symbol("import-file"), String(path), RightParen
        if i + 3 < tokens.len()
            && matches!(tokens[i].0, Token::LeftParen)
            && matches!(tokens[i + 1].0, Token::Symbol("import-file"))
            && matches!(tokens[i + 3].0, Token::RightParen)
        {
            if let Token::String(path) = &tokens[i + 2].0 {
                if let Some(norm) = normalize_import_path(path) {
                    let form_start = tokens[i].1;
                    let form_end = tokens[i + 3].1 + tokens[i + 3].2;
                    let replacement = match norm {
                        ImportNorm::Source(name) => format!("(import \"{}\")", name),
                        ImportNorm::Native(name) => format!("(import-native \"{}\")", name),
                    };
                    edits.push(Edit {
                        byte_offset: form_start,
                        byte_len: form_end - form_start,
                        replacement,
                    });
                    i += 4;
                    continue;
                }
            }
        }
        i += 1;
    }
    Ok(edits)
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
