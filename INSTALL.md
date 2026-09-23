# Installing Elle

<!-- audited: 2026-09-22 -->

What to install, how to build Elle and its plugins, and how to run the tests.

## Requirements

**Rust:** stable toolchain, edition 2021. Install via [rustup](https://rustup.rs/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**A C compiler and `make`.** The default `ffi` feature builds its own copy of
libffi from source.

**System libraries**, opened at run time by the standard modules that name
them:

| Library | Debian/Ubuntu | Gentoo | Used by |
|---------|--------------|--------|---------|
| libsqlite3 | `libsqlite3-dev` | `dev-db/sqlite` | `std/sqlite` |
| libz | `libz-dev` | `sys-libs/zlib` | `std/compress` |
| libzstd | `libzstd-dev` | `app-arch/zstd` | `std/compress` |
| libgit2 | `libgit2-dev` | `dev-libs/libgit2` | `std/git` |

**Test runner** (optional, for `make smoke` / `make test`):

| Tool | Debian/Ubuntu | Gentoo | Purpose |
|------|--------------|--------|---------|
| GNU parallel | `parallel` | `sys-process/parallel` | Parallel test execution |
| Redis | `redis-server` | `dev-db/redis` | Redis library tests |

### Debian/Ubuntu one-liner

```sh
sudo apt-get install -y build-essential libsqlite3-dev libz-dev libzstd-dev libgit2-dev parallel
```

### Gentoo one-liner

```sh
emerge dev-db/sqlite sys-libs/zlib app-arch/zstd dev-libs/libgit2 sys-process/parallel
```

## Building

```sh
# Debug build (fast to compile, slow to run)
cargo build -p elle

# Release build
cargo build --release -p elle

# Release build with WASM backend
cargo build --release -p elle --features wasm
```

The binary is at `target/debug/elle` or `target/release/elle`.

## Optional: WASM backend

The WASM backend compiles Elle code to WebAssembly and runs it under Wasmtime.
It was set aside during the memory rewrite: it frees no memory and fails
about half of the test corpus ([docs/impl/wasm.md](docs/impl/wasm.md)).
Enable it with `--features wasm`:

```sh
cargo build --release -p elle --features wasm
```

No additional system dependencies — Wasmtime is compiled from source as
a Cargo dependency.

## Optional: MLIR backend

The MLIR backend requires LLVM 22 with MLIR support. This is
experimental and not needed for normal use.

### Debian/Ubuntu

```sh
# Add LLVM apt repository
wget -qO- https://apt.llvm.org/llvm-snapshot.gpg.key | sudo tee /etc/apt/trusted.gpg.d/apt.llvm.org.asc
echo "deb http://apt.llvm.org/$(lsb_release -cs)/ llvm-toolchain-$(lsb_release -cs)-22 main" \
  | sudo tee /etc/apt/sources.list.d/llvm.list
sudo apt-get update
sudo apt-get install -y llvm-22-dev libpolly-22-dev mlir-22-tools libmlir-22-dev libclang-22-dev clang-22
```

### Environment variables

```sh
export MLIR_SYS_220_PREFIX=/usr/lib/llvm-22
export TABLEGEN_220_PREFIX=/usr/lib/llvm-22
export LIBCLANG_PATH=/usr/lib/llvm-22/lib
```

### Build

```sh
cargo build --release -p elle --features mlir
```

## Plugins

Plugins live in a [separate repository](https://github.com/elle-lisp/plugins),
checked out here as the `plugins` submodule. Each plugin depends on the
`elle-plugin` crate by the path `../../elle-plugin`, so build plugins inside an
Elle checkout:

```sh
git submodule update --init plugins
make plugins        # the portable plugins
make plugins-all    # every plugin in the workspace
```

The `.so` files land in `target/release/`, where `(import "plugin/name")`
looks for them. A plugin built anywhere else needs `--path` or `ELLE_PATH`.
[docs/plugins.md](docs/plugins.md) gives the search order and the list of
plugins.

## Testing

| Command | Runtime | What it does |
|---------|---------|-------------|
| `cargo test -p elle --lib` | ~1.5 min | Rust unit tests |
| `make smoke` | ~30 min, release | The Elle corpus under the VM and the JIT, the doctests and the embedding demos |
| `make test` | smoke + ~5 min | smoke, the corpus on the thread-pool backend, QA, and the Rust unit and integration tests |

Give the corpus the release binary; the debug default takes hours:

```sh
make smoke-elle ELLE=./target/release/elle CARGO_PROFILE=--release
```

[CONTRIBUTING.md](CONTRIBUTING.md) holds the full table.

## Submodule checkout

The repository carries two submodules: `plugins` and `mcp`, the
[MCP server](https://github.com/elle-lisp/mcp). Neither is needed to build
Elle. To populate them:

```sh
git clone --recurse-submodules https://github.com/elle-lisp/elle
# or, if already cloned:
git submodule update --init
```
