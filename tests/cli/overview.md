# The command line's tests

<!-- audited: 2026-11-23 -->

Elle programs that run the `elle` binary as a subprocess and assert on what each start prints, exits with, and leaves behind.

A flag, an exit code, the help text, a subcommand, or anything `main` decides
before a program runs is a property of the binary, so a test of it starts the
binary rather than calling the library. Such a test belongs here, in Elle. So
does a test of a repository script that starts the binary, such as the commit
hook or the bytecode parity check. Write it in Rust only when it needs a Rust type
([testing](../../docs/analysis/testing.md)). A test that starts `elle test`
belongs to [the runner's tests](../runner/overview.md) instead.

Each file opens with a call-out that names its subject. The implementation
suite runs every `.lisp` file at the top of this directory on the rig (`make
smoke-impl`), so a new file joins the pass with no list to edit. Each file
spawns the binary the harness names: `ELLE`, else the release build, else the
debug one. The commit hook's tests put that binary first on `PATH`, because
the hook runs the `elle` it finds there.

Each file splices the runner's [harness](../runner/lib/harness.lisp) with
`include-file` ([include](../../docs/include.md)). It starts the binary with an
argv, an environment, a working directory and a standard input, and answers
the exit code and both streams. A start that may never end names a deadline,
and the harness kills it there and answers a nil exit code. Every scenario writes under one scratch
directory, so a run leaves nothing behind.

Run one file by hand on the release binary:

```sh
ELLE=./target/release/elle ./target/release/elle tests/cli/argv.lisp
```
