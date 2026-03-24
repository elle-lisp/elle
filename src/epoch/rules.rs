//! Epoch migration rule definitions.
//!
//! Each breaking change to Elle increments the epoch counter and adds
//! migration rules here. Rules are pure data so they can be consumed
//! by both the in-pipeline transformer and the `elle rewrite` CLI tool.

use std::collections::HashMap;

/// Current language epoch. Bump this when making a breaking change
/// and add a corresponding entry to `MIGRATIONS`.
pub const CURRENT_EPOCH: u64 = 5;

/// A set of changes introduced at a given epoch.
#[derive(Debug, Clone)]
pub struct Migration {
    /// The epoch these rules migrate TO (from epoch - 1).
    pub epoch: u64,
    /// Human-readable summary for changelogs and error messages.
    pub summary: &'static str,
    /// The individual rules in this migration.
    pub rules: &'static [MigrationRule],
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
    /// Replace a call form structurally. Matches `(symbol arg1 ... argN)`
    /// by head symbol and arity, then rewrites using a template with
    /// positional placeholders `$1`, `$2`, etc.
    Replace {
        symbol: &'static str,
        arity: usize,
        template: &'static str,
    },
}

/// All registered migrations, ordered by epoch.
///
/// When bumping [`CURRENT_EPOCH`], add a new entry here describing
/// the breaking changes. Renames are applied mechanically; removals
/// produce compile errors that tell the user what to do instead;
/// replacements rewrite call forms structurally using templates.
static MIGRATIONS: &[Migration] = &[
    Migration {
        epoch: 1,
        summary: "consolidate assertion helpers into (assert ...)",
        rules: &[
            MigrationRule::Replace {
                symbol: "assert-true",
                arity: 2,
                template: "(assert $1 $2)",
            },
            MigrationRule::Replace {
                symbol: "assert-false",
                arity: 2,
                template: "(assert (not $1) $2)",
            },
            MigrationRule::Replace {
                symbol: "assert-eq",
                arity: 3,
                template: "(assert (= $1 $2) $3)",
            },
            MigrationRule::Replace {
                symbol: "assert-equal",
                arity: 3,
                template: "(assert (= $1 $2) $3)",
            },
            MigrationRule::Replace {
                symbol: "assert-string-eq",
                arity: 3,
                template: "(assert (= $1 $2) $3)",
            },
            MigrationRule::Replace {
                symbol: "assert-list-eq",
                arity: 3,
                template: "(assert (= $1 $2) $3)",
            },
            MigrationRule::Replace {
                symbol: "assert-not-nil",
                arity: 2,
                template: "(assert (not (nil? $1)) $2)",
            },
            MigrationRule::Replace {
                symbol: "assert-err",
                arity: 2,
                template: "(let (([ok? _] (protect ($1)))) (assert (not ok?) $2))",
            },
            MigrationRule::Replace {
                symbol: "assert-err-kind",
                arity: 3,
                template: "(let (([ok? err] (protect ($1)))) (assert (not ok?) $3) (assert (= (get err :error) $2) $3))",
            },
        ],
    },
    Migration {
        epoch: 2,
        summary: "print→println, newline→println, drop write",
        rules: &[
            MigrationRule::Rename {
                old: "print",
                new: "println",
            },
            MigrationRule::Rename {
                old: "newline",
                new: "println",
            },
            MigrationRule::Remove {
                symbol: "write",
                message: "use (pp ...) for literal form or (port/write port data) for port I/O",
            },
        ],
    },
    Migration {
        epoch: 3,
        summary: "display→print",
        rules: &[
            MigrationRule::Rename {
                old: "display",
                new: "print",
            },
        ],
    },
    Migration {
        epoch: 4,
        summary: "stream/{read,read-line,read-all,write,flush} → port/...",
        rules: &[
            MigrationRule::Rename {
                old: "stream/read-line",
                new: "port/read-line",
            },
            MigrationRule::Rename {
                old: "stream/read",
                new: "port/read",
            },
            MigrationRule::Rename {
                old: "stream/read-all",
                new: "port/read-all",
            },
            MigrationRule::Rename {
                old: "stream/write",
                new: "port/write",
            },
            MigrationRule::Rename {
                old: "stream/flush",
                new: "port/flush",
            },
        ],
    },
    Migration {
        epoch: 5,
        summary: "add→put for sets, string-contains?→has?, string/contains?→has?",
        rules: &[
            MigrationRule::Replace {
                symbol: "add",
                arity: 2,
                template: "(put $1 $2)",
            },
            MigrationRule::Rename {
                old: "string-contains?",
                new: "has?",
            },
            MigrationRule::Rename {
                old: "string/contains?",
                new: "has?",
            },
        ],
    },
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_empty_range() {
        let renames = collapsed_renames(0, 0);
        assert!(renames.is_empty());
    }

    #[test]
    fn test_renames_through_current() {
        let renames = collapsed_renames(0, CURRENT_EPOCH);
        // epoch 2: print→println, newline→println
        // epoch 3: display→print
        // epoch 4: stream/{read,read-line,read-all,write,flush} → port/...
        assert_eq!(renames.get("print"), Some(&"println"));
        assert_eq!(renames.get("newline"), Some(&"println"));
        assert_eq!(renames.get("display"), Some(&"print"));
        assert_eq!(renames.get("stream/read-line"), Some(&"port/read-line"));
        assert_eq!(renames.get("stream/read"), Some(&"port/read"));
        assert_eq!(renames.get("stream/read-all"), Some(&"port/read-all"));
        assert_eq!(renames.get("stream/write"), Some(&"port/write"));
        assert_eq!(renames.get("stream/flush"), Some(&"port/flush"));
        // epoch 5: string-contains?→has?, string/contains?→has?
        assert_eq!(renames.get("string-contains?"), Some(&"has?"));
        assert_eq!(renames.get("string/contains?"), Some(&"has?"));
        assert_eq!(renames.len(), 10);
    }

    #[test]
    fn test_replace_rules_empty_range() {
        let replaces = replace_rules_in_range(0, 0);
        assert!(replaces.is_empty());
    }

    #[test]
    fn test_replace_rules_epoch_1() {
        let replaces = replace_rules_in_range(0, 1);
        assert_eq!(replaces.len(), 9);
        // First rule should be assert-true
        assert_eq!(replaces[0].0, "assert-true");
    }

    #[test]
    fn test_removals_epoch_2() {
        let removals = removals_in_range(0, CURRENT_EPOCH);
        assert!(removals.contains_key("write"));
        assert_eq!(removals.len(), 1);
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
