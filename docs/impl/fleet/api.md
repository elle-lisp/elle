# Fleet: the client

<!-- audited: 2026-10-21 -->

The calls `std/fleet` offers, how a failed task reaches its caller, why plain
`map` dispatches on data, and the in-process backend.

[The fleet design](../fleet.md) owns the argument this serves. Nothing here is
implemented.

## The calls

| Call | Does |
|------|------|
| `(fleet:connect url)` | bind the module to a coordinator |
| `(fleet:run f & args)` | execute remotely, block, return |
| `(fleet:run-later f & args)` | fire and forget; returns the task key |
| `(fleet:map f items)` | parallel map, results in order |
| `(fleet:map-limited f items n)` | bounded parallelism |
| `(fleet:map-later f items)` | returns the job id at once |
| `(fleet:attach job-id)` | resume collecting a job's results |
| `(fleet:push queue f & args)` | a durable work queue |
| `(fleet:results queue)` | an iterator over completions |
| `(fleet:errors queue-or-job)` | an iterator over error records |
| `(fleet:pause q)`, `(fleet:resume q)` | flow control on a queue |
| `(fleet:purge q)` | drop pending and delayed work |
| `(fleet:pipeline name :width n)` | ordered FIFO lanes, striped |
| `(fleet:scatter items)` | a scattered collection of references |
| `(fleet:gather sc)` | realize it: the ordered values |
| `(fleet:put value)` | a reference: upload once |
| `(fleet:freeze f)` | a fn-ref: pin `f`'s environment now |
| `(fleet:status key-or-job)` | `:pending`, `:running`, `:done` or `:failed` |
| `(fleet:fetch ref-or-task-key)` | the value behind a reference or a result |
| `(fleet:redrive task-key)` | resubmit a dead task with a fresh budget |

`f` is an ordinary value, a named function or a lambda. The client dumps the
environment image with `f` as a distinguished root, so a lambda and its
captured values ship like everything else, and one dump covers a whole
`fleet:map`.

`fleet:freeze` dumps eagerly and returns a **fn-ref**,
`{:env <hash> :entry <name>}`, which is slay's `create_reference`. A hot call
site pays the dump once and reuses the fn-ref until the session changes.

## Errors are values

A failed task yields the error struct it raised, its stack trace, and a
reference to its captured output. `(fleet:run f x)` raises it again at the call
site, with the logs attached, as slay attached remote logs to its exceptions.

Failure policy is per job, under `:on-error`. slay needed both semantics and
shipped them in different tools: one call failed fast, and its work queue split
errors from results.

- **`:cancel`**, the default for `fleet:map` and a scattered `map`, cancels the
  job at its first error. Pending tasks are dropped at claim, running ones are
  killed like deadline overruns, and completed results stay queryable. So a
  cancel discards only work not yet done.
- **`:continue`**, the default for queues and pipelines, runs every task to a
  terminal state. `fleet:results` yields the successes and `fleet:errors` the
  error records. `fleet:gather` raises the first error only after every task
  settles.

`:cancel` is the closest match to a local `map`, which raises at its first
failing element, and the scattered collection depends on that match. Both
policies are durable transitions: a cancel is a job flag the claim loop and the
workers honor, and it survives the client that set it.

## Dispatch on data, not on a tier

Elle already runs one closure on up to four tiers, and a fifth runs GPU
kernels. So the fleet could look like one more tier, with plain `map` landing
on it. The tier contract forbids it
([differential testing](../differential.md)). A tier runs the same closure on
the same heap with identical observable behavior, and the runner records any
disagreement as a bug. Distribution cannot promise that: effects land on
another machine, delivery is at least once, and a partition is a failure no tier
has. Tiers also key on the closure alone, where distribution depends on the
code, the data and the policy.

`fleet:map` should still be sugar rather than foundation. Trait dispatch lets
`first` and `length` see through any collection ([traits](../../traits.md)), and
the same mechanism lets plain `map` land elsewhere. The dispatch key is the
data:

- `(fleet:scatter items)` uploads once and returns a **scattered
  collection**: an ordinary Elle value whose elements are references and whose
  traits carry the fleet protocol.
- `(map f sc)` dispatches through the trait table. It dumps `f`'s environment,
  records a job, and returns another scattered collection, unrealized: a view of
  result references.
- `(map g (map f sc))` composes. The second map's job is the first job's
  continuation, with no gather between the stages. The collection is the job,
  so a pipeline built from plain `map` survives every crash the job records
  survive ([orchestration](orchestration.md)).
- Realization is explicit, through `(fleet:gather sc)`, or implicit: `each`,
  `reduce` and `get` pull results through the iterator protocol as they
  complete.

Local semantics never change: `(map f [1 2 3])` stays in-process on ordinary
data. The place where work distributes is visible in the code exactly once, at
the scatter. A dynamic context would capture every `map` in its extent, which
is why the key is data ([the fleet design](../fleet.md) names the rejected
alternative).

The GPU path is the same shape one level down. Each backend has an eligibility
predicate, `fn/gpu-eligible?` there and dumpability here, and neither falls back
silently ([GPU compute](../gpu.md)). A scattered `map` raises rather than
running locally.

The two compose. A fleet task is ordinary code, so it may call `gpu:map`:
scatter across machines, SPIR-V within one. The tier system still applies
inside the worker, where a hydrated closure reaches the JIT exactly as a
compiled one does ([JIT parity](../image/boot.md)).

## The local backend

`(fleet:connect :local)` swaps Redis for an in-process store and queue: the
same API and the same task lifecycle, on the local scheduler, with no network.
It is the development and test target. It is also the second implementation of
the store and queue interfaces, which keeps them from growing Redis-shaped
assumptions, as slay's local backend did.
