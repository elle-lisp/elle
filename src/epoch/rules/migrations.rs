// audited: 2026-11-24
//! Every registered migration, ordered by epoch: the data one epoch bump adds.
//!
//! docs/epochs.md

mod early;

use super::{LexicalChange, Migration, MigrationRule, TimeArg};

/// All registered migrations, ordered by epoch: those of epochs 1 to 12,
/// then [`LATER`].
pub(super) fn all() -> impl Iterator<Item = &'static Migration> {
    early::EARLY.iter().chain(LATER)
}

/// The migrations from epoch 13 on, ordered by epoch.
///
/// When bumping [`super::CURRENT_EPOCH`], add a new entry here describing
/// the breaking changes. Renames are applied mechanically; removals
/// produce compile errors that tell the user what to do instead;
/// replacements rewrite call forms structurally using templates.
static LATER: &[Migration] = &[
    Migration {
        epoch: 13,
        summary: "string escapes: \\0, \\xHH and \\u{…} read as characters, and any other \
                  escape is a read error",
        rules: &[],
        lexical: &[LexicalChange {
            name: "string-escapes",
            summary: "a string reads \\0, \\xHH from 00 to 7f, and \\u{…}, and refuses any \
                      other escape; each escape whose meaning moved becomes the text epoch 12 \
                      read from it",
        }],
    },
    Migration {
        epoch: 14,
        summary: "durations in seconds: every call that waits takes :timeout in seconds, \
                  beside a new :deadline",
        rules: &[
            MigrationRule::MillisToSeconds {
                symbols: &[
                    "port/read",
                    "port/read-line",
                    "port/read-exact",
                    "port/read-all",
                    "port/write",
                    "port/flush",
                    "port/open",
                    "port/open-bytes",
                    "port/set-options",
                    "stream/read",
                    "stream/write",
                    "stream/flush",
                    "tcp/accept",
                    "tcp/connect",
                    "tcp/connect-ip",
                    "unix/accept",
                    "unix/connect",
                    "udp/send-to",
                    "udp/recv-from",
                ],
                arg: TimeArg::Keyword,
            },
            MigrationRule::MillisToSeconds {
                symbols: &["sys/join", "os/join", "chan/select", "chan/wait-ready"],
                arg: TimeArg::Last {
                    arity: 2,
                    negative_unbounded: false,
                },
            },
            MigrationRule::MillisToSeconds {
                symbols: &["io/wait"],
                arg: TimeArg::Last {
                    arity: 2,
                    negative_unbounded: true,
                },
            },
            MigrationRule::MillisToSeconds {
                symbols: &["ev/step"],
                arg: TimeArg::Last {
                    arity: 1,
                    negative_unbounded: true,
                },
            },
            MigrationRule::MillisToSeconds {
                symbols: &["ev/shutdown"],
                arg: TimeArg::Last {
                    arity: 1,
                    negative_unbounded: false,
                },
            },
            // `(ev/step)` did not wait, and a step with no bound now does.
            MigrationRule::Replace {
                symbol: "ev/step",
                arity: 0,
                template: "(ev/step :timeout 0)",
            },
        ],
        lexical: &[],
    },
    Migration {
        epoch: 15,
        summary: "one form per signal concept: (silent!) for (silence), (bound p ||) for \
                  (silence p), squelch takes its spec first, and muffle goes",
        rules: &[
            MigrationRule::Replace {
                symbol: "silence",
                arity: 0,
                template: "(silent!)",
            },
            MigrationRule::Replace {
                symbol: "silence",
                arity: 1,
                template: "(bound $1 ||)",
            },
            // The template keeps its head, so the walk rewrites a matched form
            // once and then descends into its elements.
            MigrationRule::Replace {
                symbol: "squelch",
                arity: 2,
                template: "(squelch $2 $1)",
            },
            MigrationRule::Remove {
                symbol: "muffle",
                message: "a boundary belongs to the caller; wrap the closure with \
                          (squelch spec f) where it is called",
            },
        ],
        lexical: &[],
    },
    Migration {
        epoch: 16,
        summary: "import-file goes: (import path) for (import-file path), include of a spec \
                  goes, and a module is one form: an old module folds into its last fn form",
        rules: &[
            // A call, not a rename: `import` is a macro, so an `import-file`
            // passed as a value has no spelling and stays as written.
            MigrationRule::Replace {
                symbol: "import-file",
                arity: 1,
                template: "(import $1)",
            },
            MigrationRule::Remove {
                symbol: "include",
                message: "a compile resolves no spec; write (include-file \"path\"), the path \
                          relative to this file",
            },
            MigrationRule::FoldModule,
        ],
        lexical: &[],
    },
    Migration {
        epoch: 17,
        summary: "a def binds only in a body: one outside a body moves into a let",
        rules: &[MigrationRule::DefIntoLet],
        lexical: &[],
    },
    Migration {
        epoch: 18,
        summary: "a name exists from its def onward: a def a function reads before \
                  it moves above the read, or binds with the function in a letrec",
        rules: &[MigrationRule::DefsBeforeReads],
        lexical: &[],
    },
    Migration {
        epoch: 19,
        summary: "the syntax operations move under syntax/: syntax/->datum for \
                  syntax->datum, syntax/<-datum for datum->syntax, syntax/e for syntax-e, \
                  and the rest; the bare names and the meta/ spellings go; and \
                  if-let's else form sees every name, which no rule can carry",
        rules: &[
            rename("syntax->datum", "syntax/->datum"),
            rename("meta/syntax->datum", "syntax/->datum"),
            rename("datum->syntax", "syntax/<-datum"),
            rename("meta/datum->syntax", "syntax/<-datum"),
            rename("syntax-e", "syntax/e"),
            rename("meta/syntax-e", "syntax/e"),
            rename("syntax-pair?", "syntax/pair?"),
            rename("meta/syntax-pair?", "syntax/pair?"),
            rename("syntax-list?", "syntax/list?"),
            rename("meta/syntax-list?", "syntax/list?"),
            rename("syntax-symbol?", "syntax/symbol?"),
            rename("meta/syntax-symbol?", "syntax/symbol?"),
            rename("syntax-keyword?", "syntax/keyword?"),
            rename("meta/syntax-keyword?", "syntax/keyword?"),
            rename("syntax-nil?", "syntax/nil?"),
            rename("meta/syntax-nil?", "syntax/nil?"),
            rename("syntax->list", "syntax/->list"),
            rename("meta/syntax->list", "syntax/->list"),
            rename("syntax-first", "syntax/first"),
            rename("meta/syntax-first", "syntax/first"),
            rename("syntax-rest", "syntax/rest"),
            rename("meta/syntax-rest", "syntax/rest"),
        ],
        lexical: &[],
    },
];

/// The rule that renames `old` to `new`.
const fn rename(old: &'static str, new: &'static str) -> MigrationRule {
    MigrationRule::Rename { old, new }
}
