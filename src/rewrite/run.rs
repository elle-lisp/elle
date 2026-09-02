//! CLI entry point for `elle rewrite`.

use super::edit::{apply_edits, Edit};
use super::engine::collect_edits;
use super::rule::{RenameSymbol, RewriteRule};
use super::text::SourceText;
use crate::epoch::rules::{
    collapsed_renames, flatten_clause_rules_in_range, flatten_rules_in_range,
    lexical_changes_in_range, removals_in_range, replace_rules_in_range, unwrap_rules_in_range,
    Lexicon, CURRENT_EPOCH,
};
use crate::epoch::{check_declared_lexicon, detect_epoch_in_source};
use crate::reader::{shebang_len, Token};
use std::collections::HashMap;

/// Collect edits that respell every token whose spelling differs between
/// `source`'s lexicon and `target` (docs/impl/lexicon.md).
pub(crate) fn collect_lexical_edits(
    source: SourceText<'_>,
    target: Lexicon,
) -> Result<Vec<Edit>, String> {
    let mut edits = Vec::new();

    for token in source.tokens()? {
        if source.in_shebang(&token) {
            continue;
        }
        let replacement = source
            .lexicon
            .respell(&token.token, &target)
            .map_err(|e| format!("{}: {}", token.loc.position(), e))?;
        if let Some(replacement) = replacement {
            edits.push(Edit {
                byte_offset: token.byte_offset,
                byte_len: token.len,
                replacement,
            });
        }
    }

    Ok(edits)
}

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
            for change in lexical_changes_in_range(0, CURRENT_EPOCH) {
                println!("  lexical: {} ({})", change.name, change.summary);
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
///
/// `file_path` is used only for error messages.
pub(crate) fn rewrite_file(
    source: &str,
    file_path: &str,
) -> Result<Option<(String, usize)>, String> {
    // Detect epoch
    let epoch_info = detect_epoch_in_source(source)?;

    let file_epoch = epoch_info.as_ref().map(|info| info.epoch);

    // The rules that read this file. A declaration the prescan cannot see
    // did not choose the lexer that tokenized the file, so when the two
    // disagree about the rules there is nothing safe to rewrite.
    if let Some(epoch) = file_epoch {
        check_declared_lexicon(epoch, source, file_path)?;
    }
    let text = SourceText::new(
        source,
        file_path,
        Lexicon::for_epoch(file_epoch.unwrap_or(CURRENT_EPOCH)),
    );

    // Check for removed symbols before doing any rewrites
    if let Some(epoch) = file_epoch {
        let removals = removals_in_range(epoch, CURRENT_EPOCH);
        if !removals.is_empty() {
            check_removals(text, &removals)?;
        }
    }

    // Collect unwrap edits: (symbol (fn [] body...)) → body...
    let unwrap_edits = if let Some(epoch) = file_epoch {
        let unwraps = unwrap_rules_in_range(epoch, CURRENT_EPOCH);
        if unwraps.is_empty() {
            Vec::new()
        } else {
            collect_unwrap_edits(text, &unwraps)?
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
            collect_flatten_edits(text, &flattens)?
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
            collect_flatten_clause_edits(text, &flatten_clauses)?
        }
    } else {
        Vec::new()
    };

    // Normalize paren-delimited binding vectors to brackets:
    // (let (name val) ...) → (let [name val] ...)
    let binding_forms = &["let", "letrec", "let*", "if-let", "when-let", "when-ok"];
    let bracket_edits = collect_bracket_edits(text, binding_forms)?;

    // Collect replace edits (syntax-level, whole-form rewrites)
    let replace_edits = if let Some(epoch) = file_epoch {
        let replaces = replace_rules_in_range(epoch, CURRENT_EPOCH);
        if replaces.is_empty() {
            Vec::new()
        } else {
            collect_replace_edits(text, &replaces)?
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
    let mut edits = collect_edits(text, &rules)?;

    // Respell tokens whose lexical rules moved between the file's epoch and
    // this one. Token-level like the renames above, so the structural filter
    // below governs both (docs/impl/lexicon.md).
    edits.extend(collect_lexical_edits(text, Lexicon::current())?);

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
        edits.push(Edit {
            byte_offset: shebang_len(source),
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

mod edits;
use edits::*;

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
mod tests;
