# Test Suite

<!-- audited: 2026-09-26 -->

Where each kind of test lives, and the command that runs it.

| Suite | Location | Run with |
|-------|----------|----------|
| Language | [tests/lang](lang/overview.md) | `make smoke-lang` |
| Implementation, in Elle | [tests/impl](impl/overview.md) | `make smoke-impl` |
| Rust unit | inline `#[cfg(test)]`, [unittests](unittests/) | `cargo test -p elle --lib` |
| Rust integration | [integration](integration/), `tests/*.rs` | `cargo test --test '*'` |
| Rust property | [property](property/) | `cargo test --test lib property::` |

[docs/spec.md](../docs/spec.md) says what separates the language suite from the
implementation suite, and [docs/analysis/testing.md](../docs/analysis/testing.md)
decides where a new test goes. [AGENTS.md](AGENTS.md) holds the Rust helpers,
the registration rule and the proptest conventions.
