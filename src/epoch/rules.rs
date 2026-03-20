//! Epoch migration rule definitions.
//!
//! Each breaking change to Elle increments the epoch counter and adds
//! migration rules here. Rules are pure data so they can be consumed
//! by both the in-pipeline transformer and the `elle rewrite` CLI tool.

use std::collections::HashMap;

/// Current language epoch. Bump this when making a breaking change
/// and add a corresponding entry to `MIGRATIONS`.
pub const CURRENT_EPOCH: u64 = 0;

/// A set of changes introduced at a given epoch.
#[derive(Debug, Clone)]
pub struct Migration {
    /// The epoch these rules migrate TO (from epoch - 1).
    pub epoch: u64,
    /// Human-readable summary for changelogs and error messages.
    pub summary: &'static str,
    /// The individual rules in this migration.
    pub rules: Vec<MigrationRule>,
}

/// A single mechanical transformation.
#[derive(Debug, Clone)]
pub enum MigrationRule {
    /// Rename a symbol: all occurrences of `old` become `new`.
    Rename {
        old: &'static str,
        new: &'static str,
    },
    /// A form has been removed. Any occurrence of this symbol in head
    /// position of a list emits the provided error message.
    Remove {
        symbol: &'static str,
        message: &'static str,
    },
}

/// All registered migrations, ordered by epoch.
///
/// When bumping [`CURRENT_EPOCH`], add a new entry here describing
/// the breaking changes. Renames are applied mechanically; removals
/// produce compile errors that tell the user what to do instead.
static MIGRATIONS: &[Migration] = &[
    // Migration {
    //     epoch: 1,
    //     summary: "rename map to transform",
    //     rules: vec![
    //         MigrationRule::Rename { old: "map", new: "transform" },
    //     ],
    // },
];

/// Get all migrations for epochs in the range (from, to].
pub fn migrations_in_range(from: u64, to: u64) -> impl Iterator<Item = &'static Migration> {
    MIGRATIONS
        .iter()
        .filter(move |m| m.epoch > from && m.epoch <= to)
}

/// Collapse all renames in a range into a single lookup table.
///
/// Chains renames across epochs: if epoch 1 renames A→B and epoch 2
/// renames B→C, the collapsed table maps A→C directly.
pub fn collapsed_renames(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut table: HashMap<&'static str, &'static str> = HashMap::new();

    for migration in migrations_in_range(from, to) {
        for rule in &migration.rules {
            if let MigrationRule::Rename { old, new } = rule {
                // If something already maps to `old`, chase the chain.
                let original = table.iter().find(|(_, v)| **v == *old).map(|(k, _)| *k);

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

/// Collect all removals in a range as (symbol, message) pairs.
pub fn removals_in_range(from: u64, to: u64) -> HashMap<&'static str, &'static str> {
    let mut result = HashMap::new();
    for migration in migrations_in_range(from, to) {
        for rule in &migration.rules {
            if let MigrationRule::Remove { symbol, message } = rule {
                result.insert(*symbol, *message);
            }
        }
    }
    result
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_range() {
        let renames = collapsed_renames(0, 0);
        assert!(renames.is_empty());
    }

    #[test]
    fn test_no_migrations_beyond_current() {
        let renames = collapsed_renames(0, CURRENT_EPOCH);
        assert!(renames.is_empty());
    }

    #[test]
    fn test_rename_chaining() {
        // Simulate chained renames manually
        let mut table: HashMap<&str, &str> = HashMap::new();

        // Epoch 1: A → B
        table.insert("A", "B");

        // Epoch 2: B → C — should update A → C
        let original = table.iter().find(|(_, v)| **v == "B").map(|(k, _)| *k);
        if let Some(original) = original {
            table.insert(original, "C");
        } else {
            table.insert("B", "C");
        }

        assert_eq!(table.get("A"), Some(&"C"));
        assert!(!table.contains_key("B"));
    }
}
