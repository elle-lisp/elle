// audited: 2026-11-17
//! What `elle rewrite` prints about itself: its help, and every rule it applies (`--list-rules`).
//!
//! docs/epochs.md

use crate::epoch::placement;
use crate::epoch::rules::{
    collapsed_renames, desugar_rules_in_range, flatten_clause_rules_in_range,
    flatten_rules_in_range, lexical_changes_in_range, migrations_in_range, removals_in_range,
    replace_rules_in_range, unwrap_rules_in_range, MigrationRule, CURRENT_EPOCH,
};

/// Print every migration rule up to the current epoch, tree rules and lexical
/// changes alike.
pub(super) fn list_rules() {
    println!("Epoch migration rules (current epoch: {}):", CURRENT_EPOCH);
    if CURRENT_EPOCH == 0 {
        println!("  (none — epoch 0)");
        return;
    }
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
    for migration in migrations_in_range(0, CURRENT_EPOCH) {
        for rule in migration.rules {
            if let MigrationRule::MillisToSeconds { symbols, arg } = rule {
                println!("  seconds: {} ({})", symbols.join(", "), arg);
            }
        }
    }
    for shorthand in desugar_rules_in_range(0, CURRENT_EPOCH) {
        println!(
            "  desugar: {:?} → ({} …)",
            shorthand,
            shorthand.shorthand_form().unwrap_or("?")
        );
    }
    if placement::applies(0, CURRENT_EPOCH) {
        println!("  def-into-let: a def outside a body moves into a let");
    }
    for change in lexical_changes_in_range(0, CURRENT_EPOCH) {
        println!("  lexical: {} ({})", change.name, change.summary);
    }
}

pub(super) fn print_help() {
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
    println!("  elle rewrite --dry-run lib/*.lisp");
}
