# Fleet: the plan

<!-- audited: 2026-10-22 -->

What fleet needs from the runtime, the order its milestones land in, and the
risks still open.

[The fleet design](../fleet.md) owns the argument, and [the tests](tests.md)
the pins each milestone lands with. Nothing here is implemented.

## What fleet requires from the runtime

Fleet is pure Elle, a `lib/fleet.lisp` over existing modules: `std/redis`,
`std/process`, `std/sync`, `plugin/hash`, and the image primitives. It asks
five things of the image work ([the image plan](../image/plan.md),
[the send plan](../send/plan.md)):

1. **`image/save` with explicit extra roots**, from the environment milestone:
   the session delta, plus the call's function value as a distinguished root,
   dumped to bytes.
2. **A scoped `image/load` and an explicit unload.** Load from bytes, through
   the anonymous memory file the store milestone built, into a handle that
   shows the manifest's bindings by name. The load installs nothing globally
   and pins no process root, and the handle's release frees the region through
   the ordinary cascade. The environment milestone loads an environment into a
   session for the process's life; fleet loads and unloads per task, thousands
   of times per worker, so the scoped API is the part fleet adds.
3. **Deterministic dumps**, which have landed: task identity depends on
   byte-identical dumps of identical sessions.
4. **Trait dispatch in the sequence functions**, which has landed. `map`,
   `filter`, `fold` and the other collection operators consult a collection's
   own method, or its `:iter`, before their type checks
   ([traits](../../traits.md)). The scattered collection is its first heavy
   consumer.
5. **`image/encode` and `image/decode`**, from the send plan's bytes
   milestone: an argument and a result cross as a data image.

## Landing order

The first two milestones need no environment image. Their entries are restricted
to bindings the worker already has, from a module file both sides load. That is
a useful durable work queue on its own, and it builds no throwaway code. The
third milestone makes fleet adhoc.

slay's second system demoted the first one's hard-won features — the striper,
flow control, cache-only reads — to a phase that never came. Here each has a
numbered home.

1. **store**: references, BLAKE3 hashing, the blob store over Redis with the
   inline tier, data images at the boundary, and task keys
   ([references](store.md)).
2. **queue**: the task lifecycle over streams and consumer groups: claim, ack,
   heartbeat, reclaim, policy-driven retry with delayed redelivery, and
   dead-letter ([the queue](queue.md)). Job records with atomic barriers and
   `fleet:attach` ([orchestration](orchestration.md)). The tick as server-side
   scripts, `fleet timerd`, its lease, and the managed-scheduler recipe
   ([liveness](liveness.md)). The worker daemon, deadlines and fuel budgets,
   `:at-most-once`, pause, resume and purge, pool routing, and the local
   backend. The calls `fleet:run`, `fleet:map`, `fleet:push`, `fleet:results`
   and `fleet:errors`.
3. **adhoc**: environment images as code transport, after the image
   environment milestone: dumps with extra roots, store, fetch, a scoped load
   and unload per task, the worker image cache, fingerprint routing, and
   `fleet:freeze` ([workers](workers.md)).
4. **orchestrate**: job continuations, `fleet:pure` and `fleet:cache-only`,
   ordered pipelines, `fleet:rate-limit` and `fleet:semaphore`. The scattered
   collection: `fleet:scatter`, `fleet:gather`, plain-`map` dispatch over it,
   and lazy realization ([the client](api.md)).
5. **observe**: per-task output captured as store blobs, and error records
   with traces. Causality: every task records the task that submitted it, so a
   recursive fan-out reads as one tree. Task timelines, and `fleet:status` over
   a whole queue.
6. **schedule**: delayed execution (`fleet:run-at`, through the same `delayed`
   zset) and recurring execution. It comes late because it is separable.
7. **elastic**: `--drain` mode, backlog reporting from the tick, and the
   invoker recipe for FaaS worker pools, down to zero workers. It depends only
   on the queue milestone; it is ordered last, not gated.

## Open risks

1. **Dump cost per call site.** An environment dump is a compacting copy of the
   session delta. Measure it on a realistic REPL session. If it taxes hot call
   sites, `fleet:freeze` is the escape hatch and a dirty-tracking cache is the
   fix.
2. **Small arguments.** An image's pages start at a base-page boundary, so a
   data image of one integer spans at least two base pages, and the 1 KiB
   inline tier admits none. Measure the dump, the store and the hydration of a
   small argument before the store milestone. An immediate riding the reference
   as itself is the candidate answer.
3. **Blocking foreign calls.** A task pinned in FFI that never yields defeats
   cooperative cancellation. The crash-only fallback, worker suicide and
   reclaim, works but is blunt. Measure how often real workloads hit it before
   designing anything finer.
4. **Hydration residue.** Each load grows monotonic state: the name memo, and
   the parameter, scope and static-region counters. Distinct images bound it,
   but measure a long-lived worker for creep, and give its disk cache an
   eviction policy.
5. **Blob pressure in Redis.** A value may reach 512 MB, and memory is the
   limit long before that. The file or S3 tier must land before anyone ships
   datasets through Redis, and the reference's `:tier` field is the seam.
6. **Coordinator operations.** Redis persistence bounds safety, and fleet
   inherits it rather than managing it: `redis.conf` when self-hosted, the
   provider's durability tier when managed. Managed products range from
   cache-grade to durable, and their names do not say which. So the deployment
   documents must state the trade-offs plainly, because a durability claim that
   rests on an unstated tier is false. The ledger caps the damage of a total
   loss either way.
7. **Fan-out storms.** A recursive `fleet:map` can mint tasks geometrically.
   Per-fleet concurrency caps and queue-depth backpressure need a design pass in
   the queue milestone.
8. **No per-task memory bound.** Fuel bounds computation and the deadline
   bounds time, but a task's allocations are unbounded. One huge allocation is
   an OOM kill of the whole worker, paid by every task beside it. Pools separate
   memory-hungry work coarsely. Region accounting is the candidate fine
   mechanism: a task's regions are enumerable, so a byte budget could kill the
   task instead of the worker. It needs a runtime hook and a design pass.
