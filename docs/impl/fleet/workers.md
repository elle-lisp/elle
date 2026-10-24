# Fleet: workers and routing

<!-- audited: 2026-10-22 -->

The process that claims and runs fleet tasks, the substrates it runs on, and
how a task finds a worker that can run it.

[The fleet design](../fleet.md) owns the argument this serves, and
[the queue](queue.md) owns the streams a worker claims from. Nothing here is
implemented.

## The worker process

`elle fleet worker` starts one long-lived Elle process. It is a process system
([processes](../../processes.md)) whose supervisor owns the Redis connection,
the heartbeat, the claim loop, and the image cache on disk. The cache holds
`$ELLE_CACHE/fleet/<hash>.image`, fetched from the store on a miss.

Each task runs in that process, as a monitored process on the worker's
scheduler. Its lifecycle is load, run, unload:

1. **Load** the environment image from the cached file: a private mapping and
   one relocation pass ([images](../image.md)). The relocation count bounds the
   cost, and an environment delta is smaller than the boot graph, so a load
   costs microseconds rather than a boot.
2. **Run** the entry under the task's policy. A fuel budget bounds runaway
   computation ([runtime signals](../../runtime.md)), and the deadline fires
   through the scheduler. `process:exit :kill` cancels even a CPU-bound task,
   because fuel preemption lets the kill land; the busy-looper example in
   [processes](../../processes.md) runs that today. Withheld capabilities
   ([capabilities](../../signals/capabilities.md)) apply per task.
3. **Unload**: encode the result first, because it may reference image pages,
   and then release the image handle. The region frees through the ordinary
   cascade. A worker repeats two image operations per task, and the image
   tests already pin both: an independent second hydration in one process, and
   an explicit free under `--trace=guardfree` ([the image plan](../image/plan.md)).

A task error is a caught signal, reported as a result. A native-level crash is
outside that model: a segfault in FFI code takes the worker process down. So the
worker is **crash-only** by design. Run it under a host supervisor, let it die,
and let the queue's reclaim hand its tasks to peers.

The same answer covers the one cancellation gap. A task stuck in a blocking
foreign call that never yields cannot be killed cooperatively, so the deadline's
last resort is worker suicide and reclaim. Trusted code makes both rare, and the
protocol makes both cheap.

Tasks are fibers, so one worker interleaves many I/O-bound tasks. CPU
parallelism comes from more workers per host, or later from `sys/spawn` threads,
each hydrating the same cached image file ([booting](../image/boot.md)).

Workers are also clients. `std/fleet` is present in every worker, so a task may
call `fleet:map` itself, and a recursive fan-out falls out of the architecture.
The claim loop never blocks on child tasks, because the job barrier lives in
Redis rather than in the parent's stack. So a single-worker fleet drains
recursive jobs too.

## Worker substrates

A worker is not a kind of machine. It is any process that runs the claim loop
and reaches the control plane, and the pull model makes substrates fungible:
nothing in the protocol knows what started a worker.

- **Resident**: `elle fleet worker` under a host supervisor on machines you
  own, always claiming, its image cache warm on disk.
- **Ephemeral**: `elle fleet worker --drain` claims until the queue is empty or
  the platform deadline nears, then exits. That is the serverless shape.

Deploying the `elle` binary to a FaaS is a one-time runtime install, like
installing elle on a host; user code still arrives as images at call time. The
pulse doubles as the scaler ([liveness](liveness.md)). The tick measures
backlog, and the scheduler's invoker starts workers when backlog exists and the
registry shows none alive. An idle fleet scales to zero and costs nothing, which
is what slay's second system bought from Lambda.

The two models already agree. Crash-only is a FaaS's native shape, the
platform's execution cap is an ordinary task deadline, and a warm container
keeps the image cache. A cold one pays one store fetch, which a same-region S3
tier makes cheap.

The fingerprint does not dissolve. A FaaS deployment pins one `elle` build,
which must match the clients it serves, and routing keeps a mixed population
honest.

Scaling policy stays out of the tick. The tick reports backlog, and the invoker
decides how many workers, on which substrate, under what burst limit. Policy is
code in the invoker, not protocol.

## Routing by fingerprint

An image is valid only for a binary whose layout fingerprint matches
([the image file](../image/format.md)). That lock is the price of page-speed
code transport, and fleet accepts it. What fleet refuses is "one fleet, one
binary", so the fingerprint is a **routing key, not a gate**:

- A client enqueues every task on `tasks:<fingerprint>`, its own.
- A worker consumes the stream for its own fingerprint.
- The worker registry records each worker's fingerprint, so `fleet:status`
  can say "no compatible worker" instead of hanging.

A mixed fleet works: each binary population serves its own clients. A rolling
upgrade is a drain, where old-fingerprint tasks finish on old workers while new
clients' tasks land on new ones.

A module-entry task names a binding from a module file both sides load, so it
carries no environment image. Its arguments are data images, which carry the
same lock ([references](store.md)), so it rides its client's fingerprint stream
like any other task. There is no cross-version image story, and none is
promised: images are regenerated, never migrated.

## Routing by pool

The second routing key is the **pool**: slay's platform, as a name instead of
provisioned infrastructure. A worker serves the pools it was started with
(`elle fleet worker --pool gpu,highmem`) and advertises them in the registry. A
task carries `:pool`, default none, and lands on `tasks:<fingerprint>:<pool>`.

Resources match by name, not by a constraint solver. "This task needs a GPU"
means the task names the `:gpu` pool, and the operator decides which machines
serve it. A fn-ref can carry its pool, so a function frozen for GPU work
remembers where it runs. `fleet:status` names an unserved pool exactly as it
names an incompatible fingerprint.
