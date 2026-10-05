# The tool producers

<!-- audited: 2026-10-05 -->

Producers that drive a tool outside Elle, parse the counts it prints, and
report each count as a reading.

Each file here is a producer ([ratchet](../../docs/ratchet.md)). A ledger
under `tests/ledger` names it, and it runs in-process under `elle-rig test`,
in the producer pass beside the implementation suite. What it checks is a
tool's report, so neither suite directory holds it.

## The audit queue

[audit.lisp](audit.lisp) reads the two counts of
[the audit queue](../../docs/impl/audit.md), which owns them.

## valgrind

[valgrind.lisp](valgrind.lisp) runs four programs on the rig under memcheck:

| Subject | The program |
|---------|-------------|
| `boot and exit` | `(+ 1 2)`: the boot, the standard library and the teardown |
| `a fiber yields and resumes` | one fiber, resumed to its yield and then to its end |
| `a file read through the I/O backend` | a `slurp` of `Cargo.toml` |
| `a function the JIT compiles` | a function called 100 times, under `--trace=syncjit`, which the program asserts is compiled |

It reads two numbers from each run's report:

| Axis | Unit | What memcheck counted |
|------|------|-----------------------|
| `definitely-lost` | bytes | memory the program still held no pointer to at exit |
| `error-contexts` | contexts | the distinct sites of an error, a definite leak included |

The producer reads contexts, never the count of errors. One site can report
any number of times, and that number follows timing: a background JIT compile
or a loaded box read 56 errors from the same 3 contexts that an idle run read
as 10. A new error site raises the count of contexts, and a known site repeated
leaves it alone. `--trace=syncjit` compiles on the VM thread, so the JIT
program's report does not depend on when a background compile lands.

The producer gates itself out when `valgrind` is not on the path, and when the
rig is not a release build. A ledger row's build names no profile, so a debug
rig would read its own counts against the release rows. The four runs start
together, because the producer runs once per JIT policy, and memcheck runs a
program at a fraction of its native speed.
