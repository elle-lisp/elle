# Fleet — adhoc distributed execution over images

Design for a distributed runtime in the spirit of slay: call a function,
run it on other machines, scale to thousands of tasks with no deploy
step. The image design ([image.md](image.md)) supplies the part that made
slay hard — shipping code — so fleet reduces to a small protocol over a
content-addressed store plus the image primitives. This doc owns the
design argument. Nothing here is implemented yet; the test plan at the
end names the pins each milestone must land with.

## The problem

Batch-parallel work should feel local. Define a function, call it on ten
thousand inputs, and collect the results — without containers, YAML, or a
cluster team. slay proved the developer experience twice in Python
(`~/git/old-slay`, `~/git/slay2`): one call (`run_remotely`,
`run_parallel`) turned a local function into a distributed one, with
content-addressed data, composable retry and caching, and durable work
queues.

Python then taxed every layer of it:

- **Code transport was serialization.** dill pickles a function object,
  not a program. Pickles differ across interpreter versions and even
  across runs, imports resolve against whatever the worker happens to
  have installed, and closures over module state break silently. The
  cache key for "this code" was a hash of unstable bytes — so the cache
  could lie.
- **The environment shipped separately from the code.** Packages arrived
  via Lambda layers or container images, built and deployed out of band.
  "Works locally, fails remotely" was a permanent failure class.
- **Durability had to be outsourced.** A retry loop or a fan-out that
  lives inside one process dies with that process. slay bought
  process-independent orchestration from Step Functions — state
  transitions the platform persisted — at the price of adopting the
  whole AWS substrate.

Elle removes the first two taxes at the root. An environment image is
the code *and* its environment in one artifact: closures with their
captured values, macros, compiled templates, and compiler state. Dumps
are byte-identical for the same graph ([image.md § Dumping](image.md)),
so the image's content hash is a stable identity for "this code".
Hydration is a private mapping plus one relocation pass — page speed —
and the hydrator's input is `(fd, offset)`, designed to accept bytes
that arrive over the network via a sealed memfd. The third tax fleet
pays deliberately: every piece of orchestration state lives in the
substrate, never in a process (§ Durable orchestration).

## The idea: a task is an image, an entry, and arguments

A **task** names three things:

1. an **environment image** — the session's bindings, dumped as a delta
   over the boot image (the `environment` milestone of
   [image.md](image.md)),
2. an **entry** — which binding in that image to call,
3. **argument references** — content-addressed values.

Any machine running a compatible `elle` binary can execute it: fetch
the image from the store (or hit the local cache), load it, look up the
entry, decode the arguments, call, encode the result back into the
store, unload. The fleet is *adhoc* in the dimension that matters:
**code ships at call time** — no registry, no image build pipeline, no
deploy step; the code you just typed into a REPL ships as a few pages
of bytes. Workers are wherever the `elle` binary runs: hosts you own,
containers, or function invocations (§ Worker substrates).

```text
(def fleet ((import "std/fleet")))
(fleet:connect "redis://coordinator:6379")

(defn embed [chunk] (model:encode chunk))     # ordinary code

(fleet:map embed chunks)                       # runs fleet-wide
```

`fleet:map` dumps the session's environment image once, stores it under
its content hash, records a job, enqueues one task per chunk, and
collects results in input order. Workers that already hold that image
hash skip the fetch, and the load is a page-cache hit.

## References and the store

Everything that crosses a machine boundary is a **reference**: an
immutable struct naming content by hash.

```text
{:hash  "…"          # BLAKE3 of the encoded bytes (plugin/hash)
 :size  1024
 :codec :send        # :send | :image
 :tier  :inline      # :inline | :store
 :bytes <bytes>}     # present only for :inline
```

- **`:send`** — `SendValue` bytes, the codec `sys/spawn` and channels
  already use ([threads.md](../threads.md)). The default for arguments
  and results.
- **`:image`** — image-format bytes: an environment image for code, or a
  data-only image (the store-spike format) for large value graphs that
  deserve page-speed hydration. A later milestone; `:send` covers v1.

Small payloads (< 1 KiB) ride inline in the reference itself. Everything
else lives in the **store**, keyed by hash: `fleet:blob:<hash>` in
Redis for v1, with a filesystem or S3 tier (via `std/aws`) behind the
same two-function interface (`store-get`, `store-put`) when blobs
outgrow Redis. Content addressing gives deduplication for free: the same
argument uploads once, and ten thousand tasks sharing one environment
image store it once.

Blobs are cache entries, not records. Any blob can expire under a TTL
policy and be re-uploaded by a client that still holds the value; result
retention is a per-fleet configuration.

Expiry has named consequences — old-slay's pointer taxonomy
(`PointerVacated`, `BogusPointer`) is the field guide. A fetch whose
blob has expired fails as `:vacated`, naming the hash: a holder of the
value re-uploads and retries, and a vacated `fleet:pure` result simply
recomputes. A fetch whose bytes do not hash to their key fails as
`:corrupt` before any decode. Neither is ever a silent `nil`.

## Task identity

```text
task-key = BLAKE3(env-hash ‖ entry ‖ arg-hash₁ ‖ … ‖ arg-hashₙ)
```

Deterministic dumps make `env-hash` meaningful: the same session state
produces the same image bytes, so the same code + the same arguments =
the same task key, across machines and across days. The key is the unit
of memoization (`fleet:pure`), of in-flight deduplication (a duplicate
submission of a running task subscribes to the existing result), and of
durability (results are queryable by key long after the client exits).

This is slay's content-addressing promise, kept honestly — dill could
not deliver a stable hash for code; the image dump can.

## Durable orchestration

slay sat on Step Functions for a reason, and the reason deserves a
precise accounting before this design claims equivalence. Step
Functions provides four properties, and only one of them is the retry
loop:

1. **Exactly-once state transitions.** Every transition commits
   against a replicated event history. A crash never leaves an
   execution half-advanced — output recorded but next state never
   scheduled, or scheduled twice.
2. **A serviceful interpreter.** The machine advances, timers fire,
   and retries schedule with zero customer compute alive. AWS provides
   the pulse.
3. **The execution as a durable object.** Named, inspectable, with a
   complete event history queryable long after completion.
4. **Replicated, multi-AZ storage** under all of the above.

Note what it does *not* provide: exactly-once task execution. Lambda
invocations run at-least-once; Step Functions guarantees the
*orchestration record*, not the side effects. The target for fleet is
therefore: exactly-once transitions, at-least-once effects, durable
timers, durable history, and recoverability. Property by property:

**Transitions are atomic scripts.** Every lifecycle transition —
submit, complete, fail, redeliver, barrier-clear — is one Lua script,
executed atomically by Redis. Multi-key updates never tear, no matter
where a worker dies. `complete(task-key, result)` is one script: gate
on `SADD job:<id>:done` (a reclaimed duplicate gets 0 and stops), then
`SET NX` the result, `XACK` the delivery, decrement the barrier, append
the history event, and — only if the barrier hit zero and `HSETNX`
wins the continuation flag — enqueue the continuation. A worker can
die before the script (the task redelivers) or after it (the ack is
already durable); it cannot die *inside* it.

**Idempotence comes from content-derived keys.** Delivery is
at-least-once, so every transition must tolerate replay. The task-key
is derived from the work itself, so the guards above make a duplicate
completion a no-op: the barrier decrements once per task-key, the
continuation fires once per job, and the first result write wins.
Retry progress lives in the task record (`:attempt`, `HINCRBY`), not
in any process: the third attempt runs on a worker born after the
first two died, at `now + backoff^attempt` via the `delayed` zset,
where atomic `ZREM` makes exactly one mover win each due task.

**History is a per-job event stream.** The same scripts append each
transition to `job:<id>:log`: claimed, completed, failed, redelivered,
barrier counts — with worker id and timestamp. That is the execution
history Step Functions sells: `fleet:status` reads the materialized
records, audit and debugging replay the log, retention is a TTL.

**The ledger makes Redis rebuildable.** Before anything is enqueued,
the job spec — env hash, entry, argument references, policy,
continuation — is written as a content-addressed blob, on the durable
store tier where one is configured. Redis is then the control plane,
and a lost control plane is an inconvenience, not an amputation:
`fleet recover` reads the spec, re-derives every task-key (dumps are
deterministic, so the derivation is a pure function), checks which
results already exist, and re-submits only the gap. Recovery is
idempotent and safe to run concurrently with normal traffic. Step
Functions cannot do this — an execution is not re-derivable from its
input; its history *is* the state. Fleet's state is a function of spec
and results, so the history is advisory and the recovery substrate is
determinism itself.

**Safety and liveness, separated.** Safety — no acknowledged
transition is lost — is bounded by Redis persistence (AOF `everysec`
loses at most a second; the ledger bounds even total loss). Liveness —
timers fire, retries redeliver, schedules enqueue — needs a clock
somewhere, because fleet has no serviceful interpreter; § Liveness
owns that design and states its residual gap. In exchange for
providing its own pulse, fleet escapes the limits the service imposes:
256 KB payloads between states (references are unbounded),
25,000-event histories, one-year execution caps, and a per-transition
price that slay's own cost model showed rivaling the compute bill.

Composition still reads as ordinary code — `fleet:retry`, `fleet:pure`,
`fleet:rate-limit` wrap a function value like slay's wrappers wrapped a
reference — but each wrapper compiles down to policy fields on the
records it emits, not to a loop inside anyone's process. The in-process
form exists too (a bare retry around a sub-call, inside one task) and
the doc calls it what it is: convenience, durable only as the task
around it. Retry policy also carries `:retry-on`, an error-class
filter: a timeout can retry while a type error dead-letters at once —
old-slay composed the same distinction from `TimeoutRetrier` and
`FailureRetrier`; fleet folds it into one policy field.
Branching that Step Functions expresses in ASL `Choice`
states is a continuation that computes and enqueues the next stage —
code, not JSON — and an external completer (the task-token pattern)
is anyone holding a task-key who writes its result.

Memoization is durable by construction: `fleet:pure` consults
`result:<task-key>` before enqueueing and publishes after. Cache
busting is a `:salt` folded into the key, and `fleet:cache-only` —
slay's `everpure` — consults the cache and never enqueues, failing a
miss with a named error: read-only access to expensive results. The
two coordination wrappers, `fleet:rate-limit` and `fleet:semaphore`,
keep their state in `rate:<name>` and `sem:<name>` — token bucket and
counter — shared by every worker.

## Liveness: the tick

Exactly three duties in fleet need a clock and nothing else does:

- **redelivery** — move due entries from `delayed` into their stream;
- **reclaim** — `XAUTOCLAIM` deliveries idle past their deadline;
- **schedules** — enqueue due `run-at` and recurring tasks.

Everything else is pulse-free: heartbeat expiry is a Redis TTL,
barrier clears ride task completions, and client wake-ups are pub/sub
with a poll fallback. So the liveness gap is not "fleet needs a
service"; it is precisely that **Redis stores the timers but cannot
run a script on a clock**.

Fleet unifies the three duties into **the tick**: one maintenance
pass built from the same atomic transition scripts. The tick is
idempotent and race-safe — `ZREM` and `XAUTOCLAIM` let exactly one
ticker win each due item — so any number of processes may tick, at any
frequency, forever. Correctness never depends on who ticks; only
lateness does.

One design constraint follows, and it is what makes the pulse a
commodity: **the tick lives server-side, as Redis scripts any RESP
client can invoke.** No Elle in the invoker, no fleet code, no state —
connect, `EVALSHA`, disconnect. Due-ness is judged against the Redis
server's own clock (`TIME` inside the script), so a skewed invoker
cannot fire a timer early. `fleet timerd` is merely the resident
invoker; anything with a clock and a Redis connection is an equally
valid one. Pulse sources layer accordingly:

| Pulse source | Ticks | Worst-case timer lateness |
|---|---|---|
| workers | between claims | seconds, on any live fleet |
| waiting clients | while blocked in `run`/`attach` | same, while anyone waits |
| `fleet timerd` | every second, on your machines | ~1 s, even on an idle fleet |
| managed scheduler → thin function | on its cron | ~1 min; ~1 s sustained |
| host cron `fleet tick` | every minute | ~1 min, if timerd dies |
| next participant | on connect | unbounded, but exact |

The first two rows make a busy fleet self-pulsing. The dedicated
pulse depends on where the control plane lives:

- **Self-hosted Redis:** run timerd where Redis runs — same host,
  same supervisor. It inherits the control plane's availability and
  adds no new way for the fleet to be down: a dark host has no
  control plane to act on and so no liveness to provide. Sentinel
  deployments run one timerd beside each replica, with a lease in
  Redis (`SET NX PX`, renewed) electing the ticker; a broken lease is
  harmless, because concurrent ticking wastes work but cannot
  double-deliver.
- **Managed Redis — just the protocol:** there is no "beside Redis"
  to run on, so rent the clock too. A managed scheduler — EventBridge
  Scheduler, Cloud Scheduler, a Workers cron trigger, a k8s CronJob —
  invokes a stateless function that speaks RESP and runs the tick.
  State durability then comes from the Redis provider's replication
  and the pulse from the scheduler's SLA: both multi-AZ, with zero
  always-on machines of yours. Scheduler floors are typically one
  minute; an invocation that ticks each second for its minute buys
  second-level lateness for idle fleets, and busy fleets are already
  self-pulsing.

The managed shape deserves the slay2 contrast stated exactly. It is a
cloud dependency, but a razor-thin one: the scheduler is a pulse, not
an interpreter — no logic, no state, no payload. Every transition
still lives in the scripts; a dead scheduler loses nothing (timers
fire late until any other pulse appears); and changing providers is
rewriting a twenty-line function. slay2 adopted Lambda, S3, and Step
Functions as its runtime. Fleet, at most, rents a clock.

What Step Functions still holds after that is not durability — state
and pulse both carry provider SLAs in the managed shape — and not
serverless compute either: an ephemeral worker on a FaaS, invoked by
the pulse when backlog exists, closes that too (§ Worker substrates).
What remains is single-vendor integration: one console, one IAM
boundary, one bill. The design also refuses to narrow lateness by
weakening the timers: keyspace-expiry notifications look like a free
clock but are not one (see Rejected alternatives).

Two corollaries. Scheduled execution is only trustworthy with a
dedicated pulse, so the tick scripts and timerd land with the queue
milestone, well before the schedule milestone that leans on them. And
a cold fleet — every pulse dead — loses no state and re-arms
completely on the next connect: the first tick drains everything due,
in order, exactly once.

## The queue

Coordination lives in Redis (`std/redis`, RESP2 — streams are ordinary
commands). The substrate contract is exactly the protocol: RESP2,
`EVAL`, streams, zsets, pub/sub. Any Redis-compatible service that
speaks these qualifies — self-hosted, Sentinel, or managed cloud
Redis — and fleet never assumes it can run code on or beside the
server. Cluster-mode services shard by key slot while the transition
scripts are multi-key, so every control-plane key shares one hash tag
(`{fleet}`) and lands in one slot; blobs stay untagged and spread.
The queue and store sit behind small interfaces, and the local
backend (below) is the second implementation that keeps them honest.

| Key (prefix `fleet:`) | Type | Content |
|---|---|---|
| `blob:<hash>` | string | encoded bytes |
| `tasks:<fingerprint>` | stream | image tasks for one binary layout, group `workers` |
| `tasks:<fingerprint>:<pool>` | stream | tasks bound to a named worker pool |
| `tasks:any` | stream | module-entry tasks any worker can run |
| `lane:<pipeline>:<n>` | stream | one FIFO lane of an ordered pipeline |
| `queue:<name>` | hash | named-queue control: paused flag, caps |
| `task:<key>` | hash | policy, status, attempt, timestamps, worker id |
| `job:<id>` | hash | task-key list, remaining-counter, continuation flag |
| `job:<id>:done` | set | task-keys already counted by the barrier |
| `job:<id>:log` | stream | transition history: the durable audit trail |
| `delayed` | zset | task keys due for redelivery, scored by time |
| `dead` | list | tasks past their retry budget, with error records |
| `result:<key>` | string | result reference (or error record) |
| `done` | pub/sub | task and job completion notifications |
| `worker:<id>` | hash + TTL | fingerprint, host, capacity, heartbeat |
| `sem:<name>`, `rate:<name>` | — | distributed semaphore, token bucket |

Lifecycle: a client submits, a worker claims via `XREADGROUP`, and the
worker completes or fails — where every submit/complete/fail/redeliver
step is one of the atomic transition scripts of § Durable
orchestration, so no crash leaves the records torn. A worker that dies
mid-task stops heartbeating; peers reclaim its pending entries with
`XAUTOCLAIM` after an idle deadline. A task whose attempt count exceeds
its budget moves to `dead` with its error record — nothing is silently
dropped. And `dead` is not a grave: `fleet:redrive` resubmits a dead
task with a fresh budget — the manual re-drive old-slay's dead-letter
queues offered.

Delivery defaults to **at-least-once**. Pure tasks compose that with
memoization into effectively-once. Side-effecting tasks get
at-least-once and the doc says so; slay made the same call — and slay
also offered the opposite semantics for `run_later`, so fleet names it
as policy: `:at-most-once` sets no retry budget, and a reclaim routes
the task to `dead` as *lost* instead of redelivering it. A task like
"send the email" runs once or is recorded lost; it never runs twice.

Queues carry flow control, which old-slay's `RedPipe` proved is not a
luxury: `fleet:pause` sets a flag the claim loop honors,
`fleet:resume` clears it, and `fleet:purge` drains a queue's pending
and delayed entries in one script. All three are ordinary transitions,
recorded in the log like any other.

Ordered execution is a **pipeline**: `fleet:pipeline` stripes pushes
round-robin across `n` FIFO lanes, and each lane permits one delivery
in flight, so order holds within a lane while lanes run in parallel —
old-slay's striper. FIFO has a price the design states rather than
hides: a failing lane head blocks its lane through its retry budget,
then dead-letters, and the lane moves on.

## Workers

`elle fleet worker` starts one long-lived Elle process: a process
system ([processes.md](../processes.md)) whose supervisor owns the
Redis connection, the heartbeat, the claim loop, and the on-disk image
cache (`$ELLE_CACHE/fleet/<hash>.image`, fetched from the store on
miss).

Each task runs **in that process**, as a monitored process on the
worker's scheduler. The task lifecycle is load, run, unload:

1. **Load** the environment image from the cached file: a private
   mapping and one relocation pass. The census bounds the cost — a
   graph the size of the whole stdlib carries ~2,100 relocations
   ([image.md](image.md) risk item 2), and an environment delta is
   smaller — so load is microseconds, not a boot.
2. **Run** the entry under the task's policy: a fuel budget bounds
   runaway computation ([runtime.md](../runtime.md)), the deadline
   fires through the scheduler, and `process:exit :kill` cancels even
   a CPU-bound task — fuel preemption is what makes the kill land
   (pinned today by the busy-looper test in
   [processes.md](../processes.md)). Withheld capabilities
   ([capabilities](../signals/capabilities.md)) apply per task.
3. **Unload**: encode the result into `SendValue` bytes first — the
   result may reference image pages — then release the image handle.
   The region frees through the ordinary cascade; the image test plan
   already pins explicit free under `--trace=guardfree` and
   independent double hydration in one process, which are exactly the
   operations a worker repeats per task.

A task error is a caught signal, reported as a result. What this model
does not contain is a native-level crash — a segfault in FFI code takes
the worker process down. The worker is **crash-only** by design: run it
under a host supervisor, let it die, let the queue's reclaim hand its
tasks to peers. The same answer covers the one cancellation gap: a task
stuck in a blocking foreign call that never yields cannot be killed
cooperatively, so the deadline's last resort is worker suicide and
reclaim. Trusted code makes both rare; the protocol makes both cheap.

Because tasks are fibers, one worker interleaves many I/O-bound tasks
concurrently. CPU parallelism comes from running more workers per host
— or, later, from `sys/spawn` threads that each map the same cached
image file, the per-worker hydration [image.md](image.md) already
specifies.

Workers are also clients: `std/fleet` is present in every worker, so a
task may call `fleet:map` itself. Recursive fan-out — slay's nested
concurrency — falls out of the architecture. The claim loop never
blocks on child tasks (the job barrier lives in Redis, not in the
parent's stack), so a single-worker fleet drains recursive jobs too.

### Worker substrates

A worker is not a kind of machine. It is any process that runs the
claim loop and can reach the control plane, and the pull model makes
the substrates fungible — nothing in the protocol knows what invoked
a worker:

- **Resident** — `elle fleet worker` under a host supervisor on
  machines you own: always claiming, image cache warm on disk.
- **Ephemeral** — `elle fleet worker --drain`: claim until the queue
  is empty or the platform deadline nears, then exit. This is the
  serverless shape. Deploying the `elle` binary to a FaaS is a
  one-time runtime install — the same act as installing elle on a
  host, not a code deploy; user code still arrives as images at call
  time. The pulse doubles as the scaler: the tick already measures
  backlog, and the scheduler's invoker starts workers when backlog
  exists and the registry shows none alive. Scale-to-zero follows —
  an idle fleet costs nothing, the property slay2 bought from Lambda
  in the first place.

The mapping is close because the models already agree: crash-only is
a FaaS's native shape, the platform's execution cap is an ordinary
task deadline, a warm container keeps the image cache across
invocations, and a cold one pays a single store fetch (a same-region
S3 tier makes that cheap). The constraint that does not dissolve is
the fingerprint: a FaaS deployment pins one `elle` build, which must
match the clients it serves, and § Routing keeps mixed populations
honest.

Scaling policy stays out of the tick. The tick reports backlog; the
invoker decides how many workers, on which substrate, under what
burst limit. Policy is code in the invoker, not protocol.

## Client API

```text
(fleet:connect url)                  # bind the module to a coordinator

(fleet:run f & args)                 # execute remotely, block, return
(fleet:run-later f & args)           # fire and forget, returns task-key
(fleet:map f items)                  # parallel map, results in order
(fleet:map-limited f items n)        # bounded parallelism
(fleet:map-later f items)            # returns job-id immediately
(fleet:attach job-id)                # resume collecting a job's results

(fleet:push queue f & args)          # durable work queue
(fleet:results queue)                # iterator over completions
(fleet:errors queue-or-job)          # iterator over error records
(fleet:pause q) (fleet:resume q)     # flow control on a queue
(fleet:purge q)                      # drop pending and delayed work
(fleet:pipeline name :width n)       # ordered FIFO lanes, striped

(fleet:scatter items)                # → scattered collection of refs
(fleet:gather sc)                    # realize: ordered values

(fleet:put value)                    # → reference: upload once

(fleet:freeze f)                     # → fn-ref: pin f's environment now
(fleet:status key-or-job)            # :pending | :running | :done | :failed
(fleet:fetch ref-or-task-key)        # value behind a reference or result
(fleet:redrive task-key)             # resubmit a dead task, fresh budget
```

`f` is an ordinary value — a named function or a lambda. The client
dumps the environment image with `f` as a distinguished root, so
lambdas and their captured values ship like everything else. One dump
covers a whole `fleet:map`. `fleet:freeze` performs the dump eagerly
and returns a **fn-ref** (`{:env <hash> :entry <name>}`) — slay's
`create_reference` — so hot call sites pay the dump once and reuse it
until the session changes.

Errors propagate as values: a failed task yields the error struct the
task raised, its stack trace, and a reference to its captured output —
`(fleet:run f x)` re-raises it at the call site, logs attached, exactly
as slay attached remote logs to exceptions.

Failure policy is per job, because slay needed both semantics and
shipped them in different tools: `apply` failed fast, `WorkQueue`
split errors from results. Fleet names the fork `:on-error`:

- **`:cancel`** — the default for `fleet:map` and scattered `map` —
  cancels the job on its first error. Pending tasks are dropped at
  claim, running ones are killed like deadline overruns, and
  completed results stay queryable: a cancel discards only work not
  yet done, where slay's fail-fast threw the batch away. This is also
  the closest match to local `map`, which raises at the first failing
  element — the scattered collection's semantics claim depends on it.
- **`:continue`** — the default for queues and pipelines — runs every
  task to a terminal state: `fleet:results` yields successes,
  `fleet:errors` yields error records, and `fleet:gather` re-raises
  the first error only after all tasks settle.

Both are durable transitions like any other: a cancel is a job flag
the claim loop and the workers honor, and it survives the client that
triggered it.

## Dispatch on data, not on a tier

Elle already runs one closure on five execution tiers, so the obvious
question is why the fleet is not tier six, with plain `(map)` landing
on it. Because the tier contract is the wrong one
([differential.md](differential.md)): a tier re-executes the same
closure on the same heap with identical observable behavior, and the
runner records any disagreement as a bug. Distribution cannot sign
that contract — effects land on another machine, delivery is
at-least-once, and a partition is a failure mode no tier has. A fleet
"tier" would diverge on every effectful closure, by design. Tiers also
key on the closure alone, and distribution is a property of code
*and* data *and* policy.

The instinct is still right: `fleet:map` should be sugar, not
foundation, because Elle already owns the mechanism that lets plain
`(map)` land elsewhere — trait dispatch, the protocol that lets
`first` and `length` see through any collection
([traits](../traits.md)). The dispatch key is the data:

- `(fleet:scatter items)` uploads once and returns a **scattered
  collection**: an ordinary Elle value whose elements are references
  and whose traits carry the fleet protocol.
- `(map f sc)` dispatches through the trait table: dump `f`'s
  environment, record a job, return another scattered collection —
  *unrealized*, a view of result references.
- `(map g (map f sc))` composes: the second map's job is the first
  job's continuation, with no gather between stages. The collection
  is the job; the § Durable orchestration records are its spine, so a
  pipeline built from plain `map` survives every crash the job
  machinery survives.
- Realization is explicit — `(fleet:gather sc)` — or implicit:
  `each`, `reduce`, and `get` pull results through the iterator
  protocol as they complete.

Local semantics never change: `(map f [1 2 3])` stays in-process on
ordinary data. Where work distributes is visible in the code exactly
once, at the scatter — which is why the dispatch key is data and not
a dynamic context (see Rejected alternatives).

The symmetry with the GPU path is real, just one level down: each
backend has an eligibility predicate — `fn/gpu-eligible?` there,
dumpability with a named refusal here — and neither falls back
silently: `gpu:map` propagates ineligibility rather than quietly
running on the CPU, and a scattered `map` errors rather than quietly
running locally. The two compose instead of competing: a fleet task
is ordinary code, so it may call `gpu:map` — scatter across machines,
SPIR-V within one. And the tier system proper still applies inside
the worker, where the hydrated closure JITs exactly as a source-boot
closure would (image.md's tier-parity gate).

## Routing: fingerprints and pools

An image is valid only for a binary whose layout fingerprint matches
([image.md § Fingerprint](image.md)); that lock is the price of
page-speed code transport, and fleet does not try to relitigate it.
What fleet refuses to inherit is "one fleet, one binary". The
fingerprint is a **routing key, not a gate**:

- A client enqueues image tasks on `tasks:<fingerprint>` — its own.
- A worker consumes the stream for *its* fingerprint, plus `tasks:any`.
- The worker registry records each worker's fingerprint, so
  `fleet:status` can say "no compatible worker" instead of hanging.

A mixed fleet therefore just works: each binary population serves its
own clients. Rolling upgrades are a drain — old-fingerprint tasks
finish on old workers while new clients' tasks land on new ones.
Module-entry tasks (a named binding from a module file both sides
load — the pre-image milestones below) carry no image at all; they ride
`tasks:any` and their compatibility bar is only the wire codec. There
is still no cross-version *image* story and none is promised — images
are regenerated, never migrated.

The second routing key is the **pool** — slay's `Platform`, reborn as
a name instead of provisioned infrastructure. A worker serves the
pools it was started with (`elle fleet worker --pool gpu,highmem`) and
advertises them in the registry; a task carries `:pool` (default none)
and lands on `tasks:<fingerprint>:<pool>`. Resource matching is by
name, not by constraint solver: "this task needs a GPU" means the task
names the `:gpu` pool and the operator decides which machines serve
it. slay's sticky environments follow for free — a fn-ref can carry
its pool, so a function frozen for GPU work remembers where it runs.
`fleet:status` names an unserved pool exactly as it names an
incompatible fingerprint.

## Security and trust

An image is code; hydrating one is `dlopen`
([image.md § Hydration](image.md)). A fleet is therefore a *trusted*
boundary: every client that can enqueue a task can execute arbitrary
code on every worker. Deploy fleets on machines with one owner, secure
Redis with AUTH and TLS (`std/tls`), and treat the coordinator URL as a
credential. Capability withholding narrows blast radius; it is not a
sandbox. Multi-tenant execution of untrusted code is a non-goal.

## The local backend

`(fleet:connect :local)` swaps Redis for an in-process store and queue
— same API, same task lifecycle on the local scheduler, no network. It
is the development and test target, and it is the second implementation
of the store/queue interfaces that keeps them from growing Redis-shaped
assumptions. slay's local backend served the same two purposes.

## What fleet requires from the runtime

Fleet is pure Elle (`lib/fleet.lisp`) over existing modules
(`std/redis`, `std/process`, `std/sync`, `plugin/hash`) plus the image
primitives. The contract on the image work:

1. **`image/save` with explicit extra roots** (environment milestone) —
   the session delta plus the call's function value as a distinguished
   root, dumped to bytes.
2. **Scoped `image/load` and explicit unload.** Load from bytes (the
   store milestone's memfd input) into a *handle* that exposes the
   manifest's bindings by name — no global installation, no
   process-root pin — and release the handle when the task ends, so
   the region frees through the ordinary cascade. This extends the
   environment milestone's API: [image.md](image.md) loads an
   environment into a session for the process's life; fleet loads and
   unloads per task, thousands of times per worker. The underlying
   operations are already pinned (explicit free under guardfree,
   independent double hydration); the delta is the scoped API.
3. **Deterministic dumps** (already specified) — task identity depends
   on byte-identical dumps of identical sessions.
4. **Trait dispatch in the sequence functions** (elle-lisp/elle#1005).
   `map`, `filter`, `reduce`, `each`, and the rest of the cascade
   family end today in hardcoded type cascades and raise a type error
   on unknown collections. Each needs a trait-consulting arm before
   that error, as `first`, `rest`, and `length` already have. This is
   independently useful — user-defined collections stop being
   second-class — and the scattered collection (§ Dispatch on data)
   is just its first heavy consumer.

## Rejected alternatives

- **Ship source text, `eval` on the worker.** Re-pays the compile the
  image work deletes, per worker per task — and it cannot ship a
  *session*: closures capture runtime values, which source does not
  carry. slay-on-dill at least shipped values; source shipping is
  strictly weaker.
- **Ship closures via `SendValue` alone** (what `sys/spawn` does
  today). Works for one closure graph, but carries no macros, no
  modules, no compiler state; encodes per task with no deduplication;
  and its bytes are not canonical, so it cannot key a cache. `SendValue`
  keeps the role it has: small arguments and results.
- **A subprocess per task.** Kill-on-timeout and crash isolation for
  free, but it pays a process spawn and a boot floor per task, and it
  forfeits what the image mechanism already guarantees in-process:
  scoped load, cascade unload, leak accounting back to baseline. Fuel
  preemption plus `process:exit :kill` covers cancellation without it;
  the crash-only worker covers native faults. A subprocess executor
  would also make the worker's own concurrency (many I/O-bound tasks
  interleaved on one scheduler) an inter-process problem.
- **Container images for code transport.** Solves environment drift by
  shipping gigabytes through a registry and a build step — the deploy
  step fleet exists to delete. An environment image is the same idea at
  the right granularity: pages, not filesystems.
- **Lambda + Step Functions as the substrate** (slay2's path).
  Provisioned serverless dictated slay2's storage, orchestration, and
  limits. The durable-orchestration property it bought is kept — as
  substrate data (§ Durable orchestration) — without the platform.
  Rejected as substrate, not as vendor: managed Redis is a
  first-class control plane and a managed scheduler a first-class
  pulse (§ Liveness) — services consumed through one protocol and one
  cron contract, not a platform built into. The store's S3 tier and a
  cloud worker pool remain open extensions behind the same
  interfaces.
- **A dynamic executor parameter as the dispatch key** (slay's
  `with platform` pattern): `(parameterize [*executor* …] (map f
  items))`. It reads well until a library call inside the extent maps
  over its own three-element list and ships it to the cluster — an
  ambient context captures every `map`, not the one you meant.
  Parameters keep their role for *policy* (deadlines, fleet
  selection, withheld capabilities); the dispatch key stays on the
  data, where `scatter` marks it exactly once.
- **Keyspace-expiry notifications as the clock.** Set a TTL key per
  timer, subscribe to expiry events, redeliver on the event. It
  inverts the durability: pub/sub delivers only to subscribers alive
  at that instant, and Redis expires keys lazily, so a missed event is
  a lost timer. The `delayed` zset entry persists until a tick
  consumes it — late is recoverable, lost is not. Notifications could
  only ever be a latency garnish, and a one-second timerd makes the
  garnish pointless.
- **A brokerless mesh (ZMQ) instead of Redis.** Buys peer-to-peer
  transport at the cost of rebuilding durability, discovery, and
  reclaim — exactly what Redis streams provide in a few commands. Redis
  is ambient infrastructure here, and the interfaces stay narrow enough
  to revisit.

## Landing order

Milestones 1–2 need no image machinery: entries are restricted to
bindings the worker already has (a module file both sides load), which
is a useful durable work queue on its own and builds no throwaway code.
Milestone 3 makes it adhoc. One more slay lesson shapes the list:
slay2's rewrite demoted the first system's hard-won features — the
striper, flow control, cache-only reads — to a "Phase 2" that never
came. Here each has a numbered home.

1. **store** — references, BLAKE3 hashing, blob store over Redis with
   the inline tier, `SendValue` encode/decode at the boundary, task
   keys.
2. **queue** — task lifecycle over streams and consumer groups: claim,
   ack, heartbeat, reclaim, policy-driven retry with delayed
   redelivery, dead-letter; job records with atomic barriers and
   `fleet:attach`; the tick as server-side scripts any RESP client
   can invoke, `fleet timerd`, its lease, and the managed-scheduler
   invoker recipe; the worker daemon; deadlines and fuel budgets;
   at-most-once policy; pause/resume/purge; pool routing; the local
   backend; `fleet:run`, `fleet:map`,
   `fleet:push`/`fleet:results`/`fleet:errors`.
3. **adhoc** — environment images as code transport (after image.md's
   environment milestone): dump-with-roots, store, fetch, scoped
   load/unload per task, the worker image cache, fingerprint routing,
   `fleet:freeze`.
4. **orchestrate** — job continuations (durable chains), `fleet:pure`
   memoization and `fleet:cache-only`, ordered pipelines (the
   striper), `fleet:rate-limit`, `fleet:semaphore`; the scattered
   collection (`fleet:scatter`/`fleet:gather`, plain-`map` dispatch
   over it, lazy realization), gated on the stdlib trait arm.
5. **observe** — per-task output capture as store blobs, error records
   with traces; causality: every task records the task that submitted
   it, so a recursive fan-out reads as one tree (slay's linked request
   ids); task timelines, `fleet:status` over a whole queue.
6. **schedule** — delayed (`fleet:run-at`, the same `delayed` zset) and
   recurring execution; late because slay's history shows it is
   separable.
7. **elastic** — `--drain` mode, backlog reporting from the tick, and
   the invoker recipe for FaaS worker pools; scale-to-zero. Depends
   only on the queue milestone; ordered last, not gated.

## Test plan

- Store: round-trip every `SendValue`-encodable type through the blob
  store; inline/store tier cutover at the size boundary; identical
  values collapse to one blob. Counter-factual: two distinct values
  never share a hash entry.
- Queue: a task submitted before any worker exists completes when one
  arrives; a killed worker's claimed task is reclaimed and finishes
  elsewhere; a task failing more than its retry budget lands in
  dead-letter with its error record, exactly once.
- Vacated blobs: expire a result blob and fetch it — the error is
  `:vacated`, naming the hash; a client holding the value re-uploads
  and the fetch succeeds; a vacated `fleet:pure` result recomputes.
  Flip one byte in a stored blob and the fetch fails `:corrupt` before
  any decode.
- Durable retry: kill the worker between attempt one and two; a fresh
  worker honors the remaining budget and the recorded backoff — the
  attempt counter lives in the substrate, not in any process.
- At-most-once: kill a worker mid-task under `:at-most-once`; the
  reclaim routes the task to `dead` as lost, never back to the stream
  — a side-effect counter reads at most one, where redelivery would
  read two.
- Flow control: `fleet:pause` stops claims while submissions still
  enqueue; `fleet:resume` drains the backlog; `fleet:purge` empties
  pending and delayed entries and the log records all three.
- Pipelines: with width two and a failing task at one lane's head,
  that lane holds order behind the retries and dead-letters the head
  while the other lane proceeds — order within a lane, parallelism
  across lanes.
- Partial failure, `:continue`: a job with one failing task among a
  hundred completes the other ninety-nine; `fleet:results` yields
  them, `fleet:errors` yields the one record, and `fleet:gather`
  re-raises only after every task reached a terminal state.
- Cancel, `:on-error :cancel`: the first failure cancels the job —
  pending tasks never run (a side-effect counter holds at the
  pre-cancel count), running tasks die like deadline overruns,
  completed results stay fetchable, and the cancel survives the
  client that triggered it. A dead task re-driven with
  `fleet:redrive` completes with a fresh budget.
- Jobs: kill the client mid-`fleet:map`; `fleet:attach` from a fresh
  process collects the full ordered results. Two workers completing a
  job's last two tasks concurrently fire the continuation exactly once
  — pinned against the atomic decrement.
- Transition atomicity: a reclaimed task completed by both its old and
  new worker decrements the barrier once and records one result — the
  counter-factual is the unguarded script, which reads a remaining
  count of −1. N concurrent tickers draining the same due entry
  redeliver it once. The job log replays to the same terminal state
  the records show.
- Liveness: with zero workers, a timerd alone redelivers a due retry
  and enqueues a due schedule — both visible in the records — and the
  work completes when a worker joins. Kill the timerd holding the
  lease; a standby's next tick takes the lease and the pending timer
  fires once. Stop every pulse with timers due, start one worker, and
  assert the first tick drains all of them, each exactly once.
- Elastic: a `--drain` worker exits on an empty queue; new backlog
  trips the invoker hook and one worker starts — pinned by driving
  the hook with a local command, no cloud involved.
- Protocol-only tick: drive the tick over a bare RESP connection —
  raw commands, no `std/fleet` in the invoker — and assert the same
  transitions a worker's tick produces. This pin is what keeps tick
  logic out of Elle-side code, where a managed-scheduler invoker
  could not reach it. Due-ness uses the server's clock: an invoker
  with a skewed clock fires nothing early.
- Recovery: flush Redis after a job completes against a durable store
  tier; `fleet recover` finds every result present and re-runs nothing
  — pinned by a side-effect counter. Flush mid-job; recovery re-submits
  only the tasks without results, and the job completes.
- Task lifecycle: a task exceeding its deadline is killed in-process
  and recorded `:timeout` — pinned with a busy-loop task that never
  yields, so the pin proves fuel preemption delivers the kill; a task
  error surfaces as an error result and the worker survives; withheld
  capabilities deny inside the task.
- Unload hygiene: run a task, unload, and assert the worker's region
  count returns to baseline; run two tasks with different images in
  sequence and assert independence; a result value survives its
  image's unload (it was encoded first).
- Adhoc: define a function in one process, `fleet:run` it in a worker
  started with no knowledge of it; a lambda capturing a local value
  produces the captured value remotely.
- Routing: a worker with a mismatched fingerprint never claims an
  image task; the task completes when a compatible worker joins;
  `fleet:status` names the incompatibility while it waits. A
  module-entry task runs on both populations. A task naming pool
  `:gpu` is claimed only by a worker serving that pool, and
  `fleet:status` names the unserved pool.
- Identity: the same session dumped twice yields one env hash; a
  changed binding changes it. `fleet:pure` executes once for identical
  submissions — pinned by a side-effect counter that would read 2 under
  re-execution.
- Durability: submit, kill the client, fetch the result by task key
  from a fresh process.
- Recursion: a task that calls `fleet:map` completes without
  deadlocking a single-worker fleet — the claim loop must not block on
  the child job's barrier.
- Scatter: `(map f (fleet:scatter items))` gathers to the same value
  as `(map f items)`. A chained map-map realizes through one
  continuation chain — pinned by asserting the client never fetches
  the intermediate stage's results. Plain `map` over an ordinary
  collection in a connected session touches neither store nor queue —
  the counter-factual for any ambient-dispatch design. The stdlib
  trait arm itself is pinned in core tests by a custom collection
  whose `:map` method returns a sentinel.
- Local backend: the whole suite above, minus reclaim and routing, runs
  against `:local` with no Redis present.

## Open risks

1. **Dump cost per call site.** An environment dump is a compacting
   copy of the session delta. Measure it on a realistic REPL session;
   if it taxes hot call sites, `fleet:freeze` is the escape hatch and a
   dirty-tracking cache is the fix.
2. **Cross-machine `SendValue`.** The codec crosses threads today, not
   builds. The symbol foundation makes symbol payloads stable;
   [image.md](image.md) risk item 4 already records two cross-table id
   holes with regression tests owed. Audit primitive references in
   argument data the same way before milestone 1 trusts the codec on
   the wire.
3. **Blocking foreign calls.** A task pinned in FFI that never yields
   defeats cooperative cancellation; the crash-only fallback (worker
   suicide + reclaim) works but is blunt. Measure how often real
   workloads hit it before designing anything finer.
4. **Hydration residue.** Per-task loads bump global monotonic state —
   name-display registries, scope and signal watermarks. Bounded by
   distinct images seen, but a long-lived worker should be measured
   for creep, and the worker's disk cache needs an eviction policy.
5. **Blob pressure in Redis.** Value cap is 512 MB but memory is not;
   the file/S3 tier must land before anyone ships datasets through
   Redis. The reference's `:tier` field is the seam.
6. **Coordinator operations.** The safety bound is Redis persistence,
   which fleet inherits rather than manages: `redis.conf` when
   self-hosted, the provider's durability tier when managed. Managed
   products span cache-grade (no persistence, async replication) to
   durable (multi-AZ, fsynced), and the names do not say which — the
   deployment docs must state the trade-offs plainly, because a
   durability claim that depends on an unstated tier is a lie. The
   ledger caps the damage of total loss either way.
7. **Fan-out storms.** Recursive `fleet:map` can mint tasks
   geometrically. Per-fleet concurrency caps and queue-depth
   backpressure need a design pass in the queue milestone.
8. **No per-task memory bound.** Fuel bounds computation and the
   deadline bounds time, but a task's allocations are unbounded: one
   huge allocation is an OOM kill of the whole worker, paid by every
   co-resident task. Pools segregate memory-hungry work coarsely.
   Region accounting is the candidate fine mechanism — a task's
   regions are enumerable, so a byte budget could kill the task
   instead of the worker — but it needs a runtime hook and a design
   pass. slay's platforms bounded memory per invocation; fleet does
   not yet.
