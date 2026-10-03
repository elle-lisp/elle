# The variant binaries

<!-- audited: 2026-09-29 -->

Two packages that build the WASM and MLIR builds as binaries of their own, each beside the rig of the same build.

`make elle-wasm` builds `elle-wasm` and `elle-rig-wasm` from
[wasm/Cargo.toml](wasm/Cargo.toml), and `make elle-mlir` builds `elle-mlir`
and `elle-rig-mlir` from [mlir/Cargo.toml](mlir/Cargo.toml). Each binary lands
in `target/release` or `target/debug`, beside `elle`, which stays the default
build. A target that runs `elle` therefore runs the default build, whichever
variant was built last. `smoke-wasm` and `smoke-mlir` run the suites on the
variant's own binaries ([testing](../docs/testing.md)).

## Why a package of its own

A binary takes its name from its package, and the root package names its
binary `elle`. A variant built as `-p elle --features mlir` wrote its binary
over the default build's, so every later target that ran `elle` ran the
variant.

Each package is a workspace of its own, and the root workspace excludes
`bins/`. Cargo resolves one set of features for all the packages one command
builds, so a member that turned `mlir` on would turn it on in every `cargo
build --workspace`, `cargo clippy --workspace` and `cargo test` of the root.
Outside the root workspace, a variant's features reach no build but its own.

## What each package holds

Each package names two binaries over the sources the root builds:
[src/main.rs](../src/main.rs) as `elle-wasm` or `elle-mlir`, and
[rig/src/main.rs](../rig/src/main.rs) as `elle-rig-wasm` or `elle-rig-mlir`.
It depends on `elle` by path with the default features off, and on `toml`, the
crate the rig reads a sidecar with.

It declares the rig's five features, `jit`, `ffi`, `uring`, `mlir` and `wasm`
([rig](../rig/overview.md)), and turns its own tier on by default. The rig and
`elle`'s help read `cfg!(feature = …)` of the package that compiles them, so a
feature the package does not declare reads as off.

A workspace of its own keeps its own `Cargo.lock` and its own profiles. Each
package commits its lockfile and copies the root's profiles, so a variant is
optimized the way the default build is.
[deps.rs](../tests/integration/deps.rs) checks each lockfile for the one
Cranelift the root's holds.

Build one by hand with the command the Makefile runs:

```sh
cargo build --release --manifest-path bins/mlir/Cargo.toml --target-dir target
```
