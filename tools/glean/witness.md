# Witnesses

<!-- audited: 2026-11-24 -->

Runs a fence's function in a child process, to confirm at run time what the release diff reads early, late or none.

[The release diff](diff.md) reads where the present releases disagree with the
spec's liveness. A reading says the two models differ, and a run says whether
the difference costs memory or frees a value still in use. `witness`
([witness.lisp](witness.lisp)) makes that run, by this rule:

> A witness runs a fence's function on each argument list beside it, in a
> child process. A none reading is witnessed when the object count grows with
> the number of calls, an early reading when a call faults under
> `--trace=guardfree` or panics, and a late reading when the object high-water
> mark of one call grows with its input size.

## The arguments

[args.lisp](args.lisp) is the table of arguments. It maps a document and a
fence's name to the argument lists that drive the fence's function down each
arm, and, for the late witness, to a function from a size to an argument list.
Both are source text, because the child compiles them beside the fence's
source. A fence the table does not name has no witness.

The child builds each argument list anew for each call, so no call sees what an
earlier call stored into its arguments. The table gives a size only where the
spec keeps no value live that grows with it, and where the input it builds
holds a fixed number of objects, such as the array `range` answers. So the
high-water mark of a correct function stays flat as the size grows.

## The child

The child runs the fence's source, binds its function to a mutable name, and
calls it through that name, so the compiler inlines nothing into the driver.
Each witness is one child, run by the binary that runs the witness
(`elle/executable`).

| Reading | The child | Witnessed when |
|---------|-----------|----------------|
| none | reads the rate of a call on the object gauge, for each argument list ([ratchet](../../docs/stdlib/ratchet.md)) | the low end of some rate's interval is half an object per call or more |
| early | calls the function 100 times with each argument list, under `--trace=guardfree` | the child's stderr reports a guardfree fault, or the child panics |
| late | reads the crest of a call, at a size of 50 | the crest is half an object per element or more |

A build with debug assertions checks the regions' counts and edges as it runs
([region diagnostics](../../docs/impl/region/diagnostics.md)). Such a build
can catch an early free at the free, before any read faults, and the child
exits 101 with a Rust panic. So an early witness takes a panic as it takes a
fault. An Elle error stays `:error`.

A leak or a hold that grows keeps whole objects, and the gauges read a shape
that keeps nothing as a tenth of an object or less. So half an object parts
the two.

## What it answers

`witness` takes the elle binary, an entry of the table with the fence's
`:source` added, and the readings to witness: any of `:none`, `:early` and
`:late`. It answers a struct from each of those readings to its verdict.

| Verdict | When |
|---------|------|
| `:witnessed` | the child shows what the rule asks |
| `:unwitnessed` | the child ran to its end and showed nothing |
| `:error` | the child failed in another way: a fence that does not compile, arguments the function refuses, or a panic outside an early witness |
| `:unsized` | a late reading for an entry with no size |

`make witness` runs [witnesses.lisp](witnesses.lisp). It runs the diff on each
fence the table names, then a witness for each reading the diff gives, and
prints each fence's readings beside their verdicts. It skips the readings of
the unit's entry function, which the child runs once and no call drives. Then it names each fence
with a reading that the table does not name.
