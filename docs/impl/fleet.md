# Fleet — adhoc distributed execution over images

<!-- audited: 2026-10-21 -->

A design for running Elle functions on other machines, with the code shipped as
an environment image at call time.

This document owns the design argument: the problem, the idea, the trust
boundary and the alternatives the design beat. Nothing here is implemented, and
none of `std/fleet` exists yet. Eight companions carry the rest:

| Document | Owns |
|----------|------|
| [references](fleet/store.md) | references, the content-addressed store, expiry, task identity |
| [orchestration](fleet/orchestration.md) | atomic transitions, idempotence, the job history, the ledger, the wrappers |
| [liveness](fleet/liveness.md) | the tick: redelivery, reclaim and schedules, and what runs it |
| [the queue](fleet/queue.md) | the Redis keys, the task lifecycle, delivery, flow control, pipelines |
| [workers](fleet/workers.md) | the worker process, its substrates, and routing by fingerprint and pool |
| [the client](fleet/api.md) | the calls, failure policy, dispatch on data, the local backend |
| [the plan](fleet/plan.md) | what fleet needs from the runtime, the landing order, the open risks |
| [the tests](fleet/tests.md) | the pins each milestone lands with |

## The problem

Batch-parallel work should feel local: define a function, call it on ten
thousand inputs, and collect the results. No containers, no YAML, no cluster
team. slay, a Python predecessor, delivered that experience. One call turned a
local function into a distributed one, with content-addressed data, composable
retry and caching, and durable work queues.

Python taxed three layers of it:

- **Code transport was serialization.** dill pickles a function object, not a
  program. A pickle differs across interpreter versions and across runs,
  imports resolve against whatever the worker has installed, and a closure over
  module state breaks silently. The cache key for "this code" hashed unstable
  bytes, so the cache could lie.
- **The environment shipped apart from the code.** Packages arrived through
  Lambda layers or container images built out of band, so "works locally,
  fails remotely" was a permanent failure class.
- **Durability was outsourced.** A retry loop or a fan-out inside one process
  dies with that process. slay bought orchestration from Step Functions, at the
  price of adopting the whole AWS substrate.

Elle removes the first two at the root. An environment image is the code and
its environment in one artifact: closures with their captured values, macros,
compiled templates and compiler state ([images](image.md)). Two dumps of one
graph are byte-identical, so an image's content hash is a stable identity for
"this code". Hydration maps pages privately and runs one relocation pass, and it
maps bytes from a sealed memory file as readily as from a path. Fleet pays the
third tax deliberately: all orchestration state lives in the substrate, never
in a process ([orchestration](fleet/orchestration.md)).

## The idea: a task is an image, an entry and arguments

A **task** names three things:

1. an **environment image**: the session's bindings, dumped as a delta over the
   boot image (the environment milestone of [the image plan](image/plan.md));
2. an **entry**: the binding in that image to call;
3. **argument references**: content-addressed values
   ([references](fleet/store.md)).

Any machine that runs a compatible `elle` binary can execute a task. It fetches
the image from the store or from its own cache, loads it, looks up the entry and
decodes the arguments. It then calls the entry, encodes the result into the
store, and unloads the image.

The fleet is adhoc where that matters: code ships at call time. There is no
registry, no image build and no deploy step, so the code typed into a REPL ships
as a few pages of bytes. Workers run wherever the `elle` binary runs
([workers](fleet/workers.md)).

A session would load the module with `(def fleet ((import "std/fleet")))` and
bind it to a coordinator with `(fleet:connect "redis://coordinator:6379")`. It
defines ordinary code, such as `(defn embed [chunk] (model:encode chunk))`, and
runs it fleet-wide with `(fleet:map embed chunks)`.

`fleet:map` dumps the session's environment image once, stores it under its
content hash, and records a job. It enqueues one task per chunk and collects the
results in input order. A worker that already holds that image hash skips the
fetch, and its load is a page-cache hit.

## Security and trust

An image is code, and hydrating one is `dlopen` ([images](image.md)). So a fleet
is a trusted boundary: every client that can enqueue a task can execute
arbitrary code on every worker. Deploy a fleet on machines with one owner,
secure Redis with AUTH and TLS (`std/tls`), and treat the coordinator URL as a
credential. Capability withholding narrows the blast radius, and it is not a
sandbox. Multi-tenant execution of untrusted code is a non-goal.

## Rejected alternatives

- **Ship source text and `eval` it on the worker.** It pays the compile the
  image deletes, per worker per task. It cannot ship a session either, because
  closures capture runtime values and source carries none.
- **Ship closures through `SendValue` alone**, as `sys/spawn` does. It carries
  one closure graph and no macros, modules or compiler state. It encodes per
  task with no deduplication, and its bytes are not canonical, so it cannot key
  a cache.
- **A subprocess per task.** Kill-on-timeout and crash isolation come free,
  but every task pays a process spawn and a boot. It also forfeits the scoped
  load, the cascade unload and the leak accounting the image mechanism
  guarantees in-process. Fuel preemption with `process:exit :kill` covers
  cancellation, and the crash-only worker covers native faults
  ([workers](fleet/workers.md)).
- **Container images for code transport.** They solve environment drift by
  shipping gigabytes through a registry and a build step, the deploy step fleet
  exists to delete. An environment image is the same idea at the right
  granularity: pages, not filesystems.
- **Lambda and Step Functions as the substrate**, slay's second path. The
  platform dictated storage, orchestration and limits. Fleet keeps the
  durability it bought, as data in the substrate. Managed Redis stays a
  first-class control plane and a managed scheduler a first-class pulse
  ([liveness](fleet/liveness.md)).
- **A dynamic executor parameter as the dispatch key**, slay's
  `with platform` pattern: `(parameterize [*executor* …] (map f items))`. A
  library call inside the extent that maps over its own three-element list ships
  that list to the cluster too. Parameters keep their role for policy, and the
  dispatch key stays on the data ([the client](fleet/api.md)).
- **Keyspace-expiry notifications as the clock.** Pub/sub delivers only to
  subscribers alive at that instant, and Redis expires keys lazily, so a missed
  event is a lost timer. A `delayed` zset entry persists until a tick consumes
  it: late is recoverable, and lost is not.
- **A brokerless mesh (ZMQ) instead of Redis.** It buys peer-to-peer transport
  and rebuilds durability, discovery and reclaim, which Redis streams provide in
  a few commands. Redis is ambient infrastructure here, and the interfaces stay
  narrow enough to revisit.
