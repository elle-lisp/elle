# Advisories

<!-- audited: 2026-11-13 -->

How the dependency audit reports an advisory, and the floor a fixed crate then holds in every lockfile.

## The audit

`Dependency Audit` runs `cargo audit` on every push to main
([main.yml](../../.github/workflows/main.yml)) and once a week
([weekly.yml](../../.github/workflows/weekly.yml)). An advisory lands against a
released version, so a lockfile that nobody touched can go from clean to
vulnerable. Both jobs continue on error, because the fix is a deliberate
dependency bump. No pull-request job runs the audit: an advisory published today
would turn every open pull request red.

`cargo audit` reads the root `Cargo.lock` alone. Each variant package under
`bins/` keeps a lockfile of its own ([bins](../../bins/overview.md)), and the
audit never reads it.

## A floor for each fixed crate

[deps.rs](../../tests/integration/deps.rs) holds a floor for each crate an
advisory names: the oldest release that carries the fix. The test fails when a
committed lockfile resolves that crate below its floor. So a `cargo update` that
goes backwards fails on the developer's machine, and a variant's lockfile fails
where the audit does not look.

A warning the audit allows gets a floor too, when a fixed release exists. That
covers an unsound API and a yanked release. A crate with no fixed release, such
as an unmaintained one, has no floor to hold.

## Clearing an advisory

1. Run `cargo update -p <crate>` in the root package, then with
   `--manifest-path bins/wasm/Cargo.toml` and `--manifest-path
   bins/mlir/Cargo.toml`.
2. Add the crate's floor to [deps.rs](../../tests/integration/deps.rs), or raise
   it, and name the advisories the floor clears.
3. Run `cargo audit` in the root, and in each package under `bins/`.

Before you raise `wasmtime`, read [cranelift](../impl/cranelift.md).
