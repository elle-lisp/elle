# The rig

<!-- audited: 2026-09-26 -->

`elle-rig` hosts the same compiler and runtime as `elle`, and configures them
in ways a user build cannot.

The implementation suite runs on the rig ([spec](../docs/spec.md) § Two
suites). A user build has one runtime per build: no flag chooses a tier, and no
flag turns the JIT off. An implementation test sometimes needs exactly that —
the interpreter alone, every function compiled on its first call, the
use-after-free oracle armed — and the rig is where those settings live. The rig
is a second executable over the `elle` library, so a program that runs under
`elle` runs under the rig unchanged. Nothing in a user build accepts a rig
setting, so none reaches user documentation.

## Running a file

```sh
elle-rig tests/impl/region-tail-move-toplevel-uaf.lisp
elle-rig --profile tests/impl/profiles/jit-eager.toml tests/lang/closures.lisp
elle-rig --print-config tests/impl/region-tail-move-toplevel-uaf.lisp
```

The rig takes every flag `elle` takes, then the rig's own two:

- `--profile PATH` applies one configuration to the file, over its sidecar.
- `--print-config` prints the configuration the file would run under, one
  `key = value` line per setting, and exits without running it.

The first argument that is not a flag names the program, and every argument
after it belongs to the program, exactly as under `elle`.

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
| `trace` | an array of trace keywords | The keywords `--trace=` accepts, `guardfree` and `scrub` among them |

The rig refuses a sidecar with a key it does not know, a value of the wrong
type, or an `mlir` key in a build with no MLIR tier. A misspelled key that the
rig ignored would run the file under the defaults and report a pass, which is
the vacuous result this suite exists to prevent.

## Profiles

A profile is a sidecar the rig applies to every file of a pass. The
implementation suite uses two, under
[tests/impl/profiles](../tests/impl/profiles/): `jit-eager.toml` runs both
suites with every function compiled on its first call, and `scrub.toml`, on
macOS, runs the language suite with each released page zeroed. A profile's
`jit` and `mlir` replace the sidecar's, and its `trace` keywords join the
sidecar's.

## How the suite drives it

`elle test --host PROGRAM` runs each file as its own child under `PROGRAM`
instead of this `elle` ([test-runner](../docs/test-runner.md) § Isolation), so
every verdict lands in the session store. The `Makefile` target `smoke-impl`
runs the implementation suite through `elle test --host
target/release/elle-rig`, then one pass per profile.

## Building it

`make elle-rig` builds the rig beside `elle`, against the same features.
`cargo build --release -p elle-rig` does the same by hand. The rig carries no
code of its own beyond reading the sidecar: the run path it drives is the
library's `elle::program`, which `elle` drives too.
