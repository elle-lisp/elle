# Source Rewriting

<!-- audited: 2026-11-17 -->

This module edits Elle source by byte span, so a rewritten file keeps its
comments, whitespace and layout.

Three callers use it:

| Caller | What it rewrites |
|--------|------------------|
| `elle rewrite` | a file, from the epoch it declares to the current one |
| `elle fmt` | the same migration, before it formats the result |
| `compile/apply-rules` | a text, by rename, replace and report rules a program supplies |

[Epochs](../../docs/epochs.md) owns the migration rules and `elle rewrite`'s
command line. [The index](AGENTS.md) holds the module's interface and data
flow.
