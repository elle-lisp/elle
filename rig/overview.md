# The rig

<!-- audited: 2026-09-29 -->

`elle-rig` hosts the same compiler and runtime as `elle`, and configures them
in ways a user build cannot.

The implementation suite runs on the rig ([spec](../docs/spec.md)). A user
build has one runtime per build: no flag chooses a tier, and no flag turns the
JIT off. An implementation test sometimes needs exactly that — the interpreter
alone, every function compiled on its first call, the use-after-free oracle
armed — and the rig is where those settings live. The rig is a second
executable over the `elle` library, so a program that runs under `elle` runs
under the rig unchanged. Nothing in a user build accepts a rig setting, so none
reaches user documentation.

## Running a file

```sh
elle-rig tests/impl/region-tail-move-toplevel-uaf.lisp
elle-rig --profile tests/impl/profiles/jit-eager.toml tests/lang/closures.lisp
elle-rig --print-config tests/impl/region-tail-move-toplevel-uaf.lisp
```

The rig takes every flag `elle` takes, then the rig's own two:

- `--profile PATH` applies one configuration to the file, over its sidecar.
- `--print-config` prints the configuration the file would run under, and exits
  without running it.

The first argument that is not a flag names the program, and every argument
after it belongs to the program, exactly as under `elle`.

The printed configuration is itself a sidecar the same build accepts, so a
failing file's configuration reproduces its run. It has one line per setting:

```toml
jit = 10
trace = ["guardfree", "scrub"]
```

`jit` prints a threshold, `"eager"` or `"off"`. `trace` prints the keywords
sorted, each once. `mlir` prints only in a build that carries the MLIR tier,
and `wasm` only in a build that carries the WebAssembly backend. A file with no
sidecar prints the build's defaults: `jit = 10` and `trace = []` in the default
build.

The rig also answers `elle`'s subcommands — `elle-rig test`, `elle-rig semver`,
`elle-rig fmt` — through the same library code `elle` calls. A program that runs
`(elle/executable)` with a subcommand, as the semver tool's tests do, therefore
runs under the rig unchanged. A subcommand reads no sidecar and no profile.

## The sidecar

A file configures the rig through a TOML file beside it with the same stem:
`region-tail-move-toplevel-uaf.lisp` reads `region-tail-move-toplevel-uaf.toml`.
A file with no sidecar runs under the defaults of the build, which is the
runtime `elle` ships.

```toml
# Arm the use-after-free oracle, and keep every function in the interpreter.
jit = "off"
trace = ["guardfree"]
```

| Key | Value | Effect |
|---|---|---|
| `jit` | `"off"`, `"eager"`, or a positive integer | The interpreter alone; compile on the first call; or compile after that many calls |
| `mlir` | the same | The MLIR tier's policy, in a build that carries it |
| `wasm` | `"off"`, `"full"`, or a positive integer | The WebAssembly backend's policy, in a build that carries it, read as `--wasm=` reads it: off; the whole program as one module; or each closure after that many calls |
| `trace` | an array of trace keywords | The build's trace keywords (`TRACE_KEYWORDS`), `guardfree` and `scrub` among them |

The rig refuses a sidecar and runs nothing when the sidecar has a key the rig
does not know, a value of the wrong type, a threshold below one, or a trace
keyword the build does not know (`TRACE_KEYWORDS`). It refuses an `mlir` key in
a build with no MLIR tier, and a `wasm` key in a build with no WebAssembly
backend. It refuses a file that is not TOML the same way. The refusal names the
key, the value, or the sidecar's file. A misspelled key that the rig ignored
would run the file under the defaults and report a pass, which is the vacuous
result this suite exists to prevent.

## Profiles

A profile is a sidecar the rig applies to every file of a pass, read with the
same rules. The implementation suite uses three, under
[tests/impl/profiles](../tests/impl/profiles/):

- [jit-eager.toml](../tests/impl/profiles/jit-eager.toml) runs both suites with every
  function compiled on its first call.
- [scrub.toml](../tests/impl/profiles/scrub.toml), on macOS, runs the language suite
  with each released page zeroed.
- [wasm-full.toml](../tests/impl/profiles/wasm-full.toml) runs the implementation
  suite on the rig of a `wasm` build, with each file compiled whole to one
  WebAssembly module ([wasm](../docs/impl/wasm.md)).

A profile's `jit`, `mlir` and `wasm` replace the sidecar's, and its `trace`
keywords join the sidecar's.

## How the suite drives it

`elle test --host PROGRAM` runs each file as its own child under `PROGRAM`
instead of this `elle` ([test-runner](../docs/test-runner.md)), so every
verdict lands in the session store. The `Makefile` target `smoke-impl` runs the
implementation suite through `elle test --host target/release/elle-rig`, then
one pass per profile. The target `smoke-wasm` runs the implementation suite on
the rig of the `wasm` build twice: under each file's sidecar, then under
`wasm-full.toml` less the files `WASM_SKIP` names.

## Building it

`make elle-rig` builds the rig beside `elle`, against the same features.
`cargo build --release -p elle-rig` does the same by hand. `make elle-wasm`
builds both with the `wasm` feature. The rig carries no code of its own beyond
reading the sidecar: the run path and the subcommands it drives are the
library's `elle::program`, which `elle` drives too.
