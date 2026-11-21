// audited: 2026-11-20
//! What `elle rewrite` prints about itself: its help, and every rule it applies (`--list-rules`).
//!
//! docs/epochs.md

use crate::epoch::rules::{migrations_in_range, MigrationRule, CURRENT_EPOCH};

/// Print every migration rule up to the current epoch, tree rules and lexical
/// changes alike, each under the epoch that adds it and that epoch's summary.
pub(super) fn list_rules() {
    println!("Epoch migration rules (current epoch: {}):", CURRENT_EPOCH);
    if CURRENT_EPOCH == 0 {
        println!("  (none — epoch 0)");
        return;
    }
    for migration in migrations_in_range(0, CURRENT_EPOCH) {
        println!();
        println!("epoch {}: {}", migration.epoch, migration.summary);
        for rule in migration.rules {
            println!("  {}", describe(rule));
        }
        for change in migration.lexical {
            println!("  lexical: {} ({})", change.name, change.summary);
        }
    }
}

/// One rule as `--list-rules` prints it: its kind, then what it rewrites.
fn describe(rule: &MigrationRule) -> String {
    match rule {
        MigrationRule::Rename { old, new } => format!("rename: {} → {}", old, new),
        MigrationRule::Remove { symbol, message } => format!("remove: {} ({})", symbol, message),
        MigrationRule::Replace {
            symbol,
            arity,
            template,
        } => format!("replace: {} (arity {}) → {}", symbol, arity, template),
        MigrationRule::Unwrap { symbol, message } => format!("unwrap:  {} ({})", symbol, message),
        MigrationRule::FlattenBindings { symbols } => format!(
            "flatten: {} (nested-pair → flat bindings)",
            symbols.join(", ")
        ),
        MigrationRule::FlattenClauses { symbols, .. } => format!(
            "flatten-clauses: {} (parenthesized → flat pairs)",
            symbols.join(", ")
        ),
        MigrationRule::Desugar { shorthand } => format!(
            "desugar: {:?} → ({} …)",
            shorthand,
            shorthand.shorthand_form().unwrap_or("?")
        ),
        MigrationRule::MillisToSeconds { symbols, arg } => {
            format!("seconds: {} ({})", symbols.join(", "), arg)
        }
        MigrationRule::FoldModule => {
            "fold-module: a module of several forms folds into its last fn form".to_string()
        }
        MigrationRule::DefIntoLet => {
            "def-into-let: a def outside a body moves into a let".to_string()
        }
        MigrationRule::DefsBeforeReads => "defs-before-reads: a def a forward read reaches \
                                           moves above the read, or binds with the reader \
                                           in a letrec"
            .to_string(),
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
