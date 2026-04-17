# Installing Elle

## Requirements

**Rust:** stable toolchain, edition 2021. Install via [rustup](https://rustup.rs/):

```sh
curl --proto '=https' --tlsv1.2 -sSf https://sh.rustup.rs | sh
```

**System libraries** (used by FFI-based standard library modules):

| Library | Debian/Ubuntu | Gentoo | Used by |
|---------|--------------|--------|---------|
| libffi | `libffi-dev` | `dev-libs/libffi` | C FFI (`ffi/` module) |
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
sudo apt-get install -y libffi-dev libsqlite3-dev libz-dev libzstd-dev libgit2-dev parallel
```

### Gentoo one-liner

```sh
emerge dev-libs/libffi dev-db/sqlite sys-libs/zlib app-arch/zstd dev-libs/libgit2 sys-process/parallel
```

## Building

```sh
# Debug build (fast compile, used by make smoke)
cargo build -p elle

# Release build
cargo build --release -p elle

# Release build with WASM backend
cargo build --release -p elle --features wasm
```

The binary is at `target/debug/elle` or `target/release/elle`.

## Optional: WASM backend

The WASM backend compiles Elle code to WebAssembly via Wasmtime. Enable
with `--features wasm`:

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

Plugins live in a [separate repository](https://github.com/elle-lisp/plugins)
and use a stable ABI — they can be compiled independently from elle.

```sh
git clone https://github.com/elle-lisp/plugins elle-plugins
cd elle-plugins
cargo build --release
```

The `.so` files appear in `target/release/`. Elle discovers them
automatically when loaded via `(import "plugin/name")`.

For local development against a checkout of elle, add a cargo config
override so plugins resolve `elle-plugin` from your local tree:

```toml
# elle-plugins/.cargo/config.toml
[patch."https://github.com/elle-lisp/elle"]
elle-plugin = { path = "../elle/elle-plugin" }
```

## Testing

```sh
make smoke    # ~30s — Elle scripts (VM + JIT + WASM) + doctests
make test     # ~3min — smoke + MCP integration + clippy + fmt + unit tests
```

## Submodule checkout

The elle repository includes plugins as a git submodule for convenience.
To populate it after cloning:

```sh
git clone --recurse-submodules https://github.com/elle-lisp/elle
# or, if already cloned:
git submodule update --init
```

The submodule is not required for building elle — it's there for
browsing and local plugin development.
