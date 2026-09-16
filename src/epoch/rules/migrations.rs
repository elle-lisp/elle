// audited: 2026-09-16
// Every registered migration, ordered by epoch: the data one epoch bump adds.
// docs/epochs.md

use super::{Migration, MigrationRule};

/// All registered migrations, ordered by epoch.
///
/// When bumping [`super::CURRENT_EPOCH`], add a new entry here describing
/// the breaking changes. Renames are applied mechanically; removals
/// produce compile errors that tell the user what to do instead;
/// replacements rewrite call forms structurally using templates.
pub(super) static MIGRATIONS: &[Migration] = &[
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
                template: "(let [[ok? _] (protect ($1))] (assert (not ok?) $2))",
            },
            MigrationRule::Replace {
                symbol: "assert-err-kind",
                arity: 3,
                template: "(let [[ok? err] (protect ($1))] (assert (not ok?) $3) (assert (= (get err :error) $2) $3))",
            },
        ],
        lexical: &[],
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
        lexical: &[],
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
        lexical: &[],
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
        lexical: &[],
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
        lexical: &[],
    },
    Migration {
        epoch: 6,
        summary: "remove ev/run from user code — runtime wraps all code in the async scheduler",
        rules: &[MigrationRule::Unwrap {
            symbol: "ev/run",
            message: "user code already runs in the async scheduler; remove the ev/run wrapper",
        }],
        lexical: &[],
    },
    Migration {
        epoch: 7,
        summary: "flat let bindings — (let [a 1 b 2] ...) instead of (let [[a 1] [b 2]] ...)",
        rules: &[MigrationRule::FlattenBindings {
            symbols: &["let", "letrec", "let*", "if-let", "when-let", "when-ok"],
        }],
        lexical: &[],
    },
    Migration {
        epoch: 8,
        summary: "var → def @; let/params immutable by default",
        rules: &[MigrationRule::Replace {
            symbol: "var",
            arity: 2,
            template: "(def @$1 $2)",
        }],
        lexical: &[],
    },
    Migration {
        epoch: 9,
        summary: "flat cond/match clauses",
        rules: &[
            MigrationRule::FlattenClauses {
                symbols: &["cond"],
                skip: 0,
            },
            MigrationRule::FlattenClauses {
                symbols: &["match"],
                skip: 1,
            },
        ],
        lexical: &[],
    },
    Migration {
        epoch: 10,
        summary: "cons→pair, car→first, cdr→rest",
        rules: &[
            MigrationRule::Rename { old: "cons", new: "pair" },
            MigrationRule::Rename { old: "car", new: "first" },
            MigrationRule::Rename { old: "cdr", new: "rest" },
        ],
        lexical: &[],
    },
    Migration {
        epoch: 11,
        summary: "sys/spawn→sys/spawn-vm, os/spawn→os/spawn-vm (sys/spawn is now the \
                  heavy, stdlib-backed worker; the old light worker is sys/spawn-vm)",
        // Pre-epoch code spawned a primitives-only worker. `sys/spawn`/`os/spawn`
        // now load the standard library (so eval/read resolve stdlib in the
        // worker) and are correspondingly heavier; the cheap primitives-only
        // worker is `sys/spawn-vm`/`os/spawn-vm`. Renaming the old qualified
        // names to their `-vm` form preserves the original (light) behavior of
        // existing code, here and in the wild. New code (this epoch) gets the
        // heavy default via `sys/spawn`.
        //
        // We deliberately do NOT rename the bare `spawn` symbol: `Rename`
        // rewrites every matching symbol (it isn't binding-aware), and `spawn`
        // is also used as a local — the `(ev/scope (fn [spawn] …))` nursery
        // param — so a global rename would clobber it. Only the qualified
        // `sys/spawn`/`os/spawn` (unambiguously the primitive) are renamed.
        // (Bare `spawn` is no longer registered as a primitive alias at all —
        // an ambiguous global was an accident waiting to happen — so it is
        // purely a local now; top-level code must use sys/spawn[-vm].)
        rules: &[
            MigrationRule::Rename { old: "sys/spawn", new: "sys/spawn-vm" },
            MigrationRule::Rename { old: "os/spawn", new: "os/spawn-vm" },
        ],
        lexical: &[],
    },
    Migration {
        epoch: 12,
        summary: "remove coroutine API — use fibers directly",
        rules: &[
            // coro/new and make-coroutine → (fiber/new fn |:yield|)
            MigrationRule::Replace {
                symbol: "coro/new",
                arity: 1,
                template: "(fiber/new $1 |:yield|)",
            },
            MigrationRule::Replace {
                symbol: "make-coroutine",
                arity: 1,
                template: "(fiber/new $1 |:yield|)",
            },
            // coro/* → fiber/* renames
            MigrationRule::Rename { old: "coro/resume", new: "fiber/resume" },
            MigrationRule::Rename { old: "coro/status", new: "fiber/status" },
            MigrationRule::Rename { old: "coro/done?", new: "fiber/done?" },
            MigrationRule::Rename { old: "coro/value", new: "fiber/value" },
            // gen-1 long-form renames
            MigrationRule::Rename { old: "coroutine-resume", new: "fiber/resume" },
            MigrationRule::Rename { old: "coroutine-status", new: "fiber/status" },
            MigrationRule::Rename { old: "coroutine-done?", new: "fiber/done?" },
            MigrationRule::Rename { old: "coroutine-value", new: "fiber/value" },
            // predicates
            MigrationRule::Rename { old: "coroutine?", new: "fiber?" },
            MigrationRule::Rename { old: "coro?", new: "fiber?" },
            // delegation
            MigrationRule::Rename { old: "yield-from", new: "yield*" },
            // removals — fibers are natively iterable
            MigrationRule::Remove {
                symbol: "coro/>iterator",
                message: "fibers are natively iterable; remove the coro/>iterator call",
            },
            MigrationRule::Remove {
                symbol: "coroutine->iterator",
                message: "fibers are natively iterable; remove the coroutine->iterator call",
            },
            MigrationRule::Remove {
                symbol: "coroutine-next",
                message: "use (fiber/resume f) instead of (coroutine-next f)",
            },
        ],
        lexical: &[],
    },
];
