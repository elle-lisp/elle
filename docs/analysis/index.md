# Analysis

<!-- audited: 2026-10-24 -->

Meta-analysis tools: testing, CI, debugging, semantic portraits, and the reports
behind the source cap and the signal solver's engines.

| File | Content |
|------|---------|
| [README](README.md) | The three layers of code understanding: portraits, the MCP graph, agent reasoning |
| [testing](testing.md) | Test strategy, decision tree, the order `make test` gates in |
| [ci](ci.md) | CI structure, local workflow, failure triage |
| [plugins](plugins.md) | How CI compiles the plugins submodule and builds the plugins the language suite loads |
| [debugging](debugging.md) | Introspection primitives, time API |
| [portrait](portrait.md) | Semantic portraits, compile/analyze |
| [agent-reasoning](agent-reasoning.md) | Portrait and SPARQL queries in an agent's refactoring workflow |
| [budget](budget.md) | Where sources start to mix subjects, and the 2,500-token cap read off it |
| [solver-engines](solver-engines.md) | Five Datalog engines on the signal solver's rules: their speed, and whether their models agree |
