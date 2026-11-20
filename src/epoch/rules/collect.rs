// audited: 2026-11-19
//! The collectors that read the migration tables for one range of epochs.
//!
//! docs/epochs.md

use super::{migrations, LexicalChange, Migration, MigrationRule, TimeArg};
use crate::reader::Token;
use std::collections::HashMap;

/// Get all migrations for epochs in the range (from, to].
pub fn migrations_in_range(from: u64, to: u64) -> impl Iterator<Item = &'static Migration> {
    migrations::all().filter(move |m| m.epoch > from && m.epoch <= to)
}

/// Every token-level change made in the range (from, to].
pub fn lexical_changes_in_range(
    from: u64,
    to: u64,
) -> impl Iterator<Item = &'static LexicalChange> {
    migrations_in_range(from, to).flat_map(|m| m.lexical.iter())
}

/// Collapse all renames in a range into a single lookup table.
///
/// Chains renames across epochs: if epoch 1 renames A→B and epoch 2
/// renames B→C, the collapsed table maps A→C directly.
pub fn collapsed_renames(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut table: HashMap<&'static str, &'static str> = HashMap::new();

    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Rename { old, new } = rule {
                // If something already maps to `old`, chase the chain.
                let original = table.iter().find(|(_, v)| *v == old).map(|(k, _)| *k);

                if let Some(original) = original {
                    table.insert(original, new);
                } else {
                    table.insert(old, new);
                }
            }
        }
    }

    table
}

/// Collect all replace rules in a range as (symbol, arity, template) tuples.
pub fn replace_rules_in_range(from: u64, to: u64) -> Vec<(&'static str, usize, &'static str)> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Replace {
                symbol,
                arity,
                template,
            } = rule
            {
                result.push((*symbol, *arity, *template));
            }
        }
    }
    result
}

/// Collect all unwrap rules in a range as (symbol, message) pairs.
pub fn unwrap_rules_in_range(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut result = HashMap::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Unwrap { symbol, message } = rule {
                result.insert(*symbol, *message);
            }
        }
    }
    result
}

/// Collect all flatten-bindings rules in a range as sets of symbols.
pub fn flatten_rules_in_range(from: u64, to: u64) -> Vec<&'static str> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::FlattenBindings { symbols } = rule {
                for sym in *symbols {
                    if !result.contains(sym) {
                        result.push(sym);
                    }
                }
            }
        }
    }
    result
}

/// Collect all flatten-clauses rules in a range as (symbol, skip) pairs.
pub fn flatten_clause_rules_in_range(from: u64, to: u64) -> Vec<(&'static str, usize)> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::FlattenClauses { symbols, skip } = rule {
                for sym in *symbols {
                    if !result.iter().any(|(s, _)| s == sym) {
                        result.push((*sym, *skip));
                    }
                }
            }
        }
    }
    result
}

/// Collect the shorthand tokens every desugar rule in a range names.
pub fn desugar_rules_in_range(from: u64, to: u64) -> Vec<Token<'static>> {
    let mut result = Vec::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Desugar { shorthand } = rule {
                result.push(shorthand.clone());
            }
        }
    }
    result
}

/// Where each call a `MillisToSeconds` rule in a range names took its
/// duration, keyed by head symbol.
///
/// A head an epoch in the range renamed is keyed by its old name too. The
/// migration sees a call before the rename reaches its head, so a file old
/// enough to write `stream/read-line` still has its duration converted.
pub fn millis_rules_in_range(from: u64, to: u64) -> HashMap<&'static str, TimeArg> {
    let renames = collapsed_renames(from, to);
    let mut result = HashMap::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::MillisToSeconds { symbols, arg } = rule {
                for sym in *symbols {
                    result.insert(*sym, *arg);
                    for (old, _) in renames.iter().filter(|(_, new)| *new == sym) {
                        result.insert(*old, *arg);
                    }
                }
            }
        }
    }
    result
}

/// Collect all removals in a range as (symbol, message) pairs.
pub fn removals_in_range(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut result = HashMap::new();
    for migration in migrations_in_range(from, to) {
        for rule in migration.rules {
            if let MigrationRule::Remove { symbol, message } = rule {
                result.insert(*symbol, *message);
            }
        }
    }
    result
}
