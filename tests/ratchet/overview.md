# The tool producers

<!-- audited: 2026-10-21 -->

Producers that drive a tool outside Elle, parse the counts it prints, and
report each count as a reading.

Each file here is a producer ([ratchet](../../docs/ratchet.md)). A ledger
under [tests/ledger](../ledger) names it, and it runs in-process under
`elle-rig test`. What it checks is a tool's report, so neither suite directory
holds it.

The audit and size producers read the tree alone, so `make tree-producers`
runs them on the release rig, and `make qa` runs that target
([ci](../../docs/analysis/ci.md)). The valgrind producer runs in each build's
producer pass beside the implementation suite.

## The audit queue

[audit.lisp](audit.lisp) reads the three counts of
[the audit queue](../../docs/impl/audit.md), which owns them.

## valgrind

[valgrind.lisp](valgrind.lisp) runs four programs on the rig under memcheck,
and runs each one on both paths a boot takes to the standard library:

| Subject | The program |
|---------|-------------|
| `boot and exit` | `(+ 1 2)`: the boot, the standard library and the teardown |
| `a fiber yields and resumes` | one fiber, resumed to its yield and then to its end |
| `a file read through the I/O backend` | a `slurp` of `Cargo.toml` |
| `a function the JIT compiles` | a recursive function called 100 times, under `--trace=syncjit`, which the program asserts is compiled. It is recursive so the graft keeps its call: a function the graft unrolls is never called, and never compiled |

| Path | The flag | What the boot does |
|------|----------|--------------------|
| `stdlib compiled` | `--cache=` | compiles the standard library, because caching is off |
| `stdlib cached` | `--cache=DIR`, a fresh directory | loads the standard library that one unmeasured run wrote to `DIR` |

A reading's subject joins the program and the path with a comma, for example
`boot and exit, stdlib cached`. It reads two numbers from each run's report:

| Axis | Unit | What memcheck counted |
|------|------|-----------------------|
| `definitely-lost` | bytes | memory the program still held no pointer to at exit |
| `error-contexts` | contexts | the distinct sites of an error, a definite leak included |

The two paths run different code, so each reads error sites the other does
not. A run with no `--cache` flag uses the shared cache directory, and that
directory holds the file of one binary at a time
([stdlib-cache](../../docs/impl/stdlib-cache.md)). Its path, and so its
reading, would turn on which binary ran last. The producer therefore names the
path of every run, and its readings do not depend on what ran before it.

The producer reads contexts, never the count of errors. One site can report
any number of times, and that number follows the work the run did: a boot
that compiles the standard library repeats its sites more often than one that
loads it. A new error site raises the count of contexts, and a known site
repeated leaves it alone. `--trace=syncjit` compiles on the VM thread, so the
JIT program's report does not depend on when a background compile lands.

The producer gates itself out when the rig is not a release build, and then
when `valgrind` is not on the path. A ledger row's build names no profile, so a
debug rig would read its own counts against the release rows. The profile is
checked first, so a debug rig gives the same reason on every box, whether or
not `valgrind` is installed. The eight measured
runs start together, because the producer runs once per JIT policy, and
memcheck runs a program at a fraction of its native speed.

## The size ledger

[size.lisp](size.lisp) reads every source and every document over the cap
[the documentation policy](../../DOCUMENTATION.md) states for its kind:

| Subject | Axis | Value |
|---------|------|-------|
| a file's path | `tokens` | its tokens, rounded up to the next multiple of 500 |
| `the excess the work sheds` | `tokens` | each file's reading less its cap, summed over the files that are not tables |
| `the excess tables carry` | `tokens` | the same sum over the tables |

A source is a line of `scripts/audit --sources`, and a document a line of
`scripts/audit --documents`, so the size ledger and the audit queue agree on
what is exempt ([the audit queue](../../docs/impl/audit.md)). A file's tokens
are `(elle/tokens text)` over its whole text ([the rig](../../rig/overview.md)).
A file at its cap or under it reads nothing.

The rows are in the `size-*.lisp` files under [tests/ledger](../ledger), split
so that each file stays under the cap. A row's subject is a path rather than a literal in the producer, so
[ledgers.rs](../integration/ledgers.rs) holds each one to a file the tree
tracks.

The producer's `tables` list admits a table, each entry a path and the rows it
holds. The list is empty.

The producer gates itself out on every build but the reference build,
`jit-uring-linux-x86_64`. A text reads the same on every build, and the rows
belong to that one.
