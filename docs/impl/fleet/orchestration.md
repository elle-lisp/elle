# Fleet: durable orchestration

<!-- audited: 2026-10-21 -->

How fleet keeps every retry, fan-out and continuation in the substrate, so no
process death loses or repeats a transition.

[The fleet design](../fleet.md) owns the argument this serves, and
[the queue](queue.md) owns the keys the scripts below write. Nothing here is
implemented.

## What Step Functions provides

slay used Step Functions for orchestration. Before fleet claims equivalence, it
has to state what that service provides. It provides four properties, and only
one of them is the retry loop:

1. **Exactly-once state transitions.** Every transition commits against a
   replicated event history. A crash never leaves an execution half-advanced:
   an output recorded and the next state never scheduled, or scheduled twice.
2. **A serviceful interpreter.** The machine advances, timers fire, and retries
   schedule with no customer compute alive. AWS provides the pulse.
3. **The execution as a durable object.** It is named and inspectable, with a
   complete event history queryable long after completion.
4. **Replicated, multi-AZ storage** under all of the above.

It does not provide exactly-once task execution. Lambda invocations run at
least once, and Step Functions guarantees the orchestration record, not the
side effects. The target for fleet is therefore exactly-once transitions,
at-least-once effects, durable timers, durable history and recoverability.

## Transitions are atomic scripts

Every lifecycle transition is one Lua script, which Redis executes atomically:
submit, complete, fail, redeliver and barrier-clear. A multi-key update never
tears, wherever a worker dies.

`complete(task-key, result)` is one script:

1. Gate on `SADD job:<id>:done`. A reclaimed duplicate gets 0 and stops.
2. `SET NX` the result.
3. `XACK` the delivery.
4. Decrement the barrier.
5. Append the history event.
6. Enqueue the continuation, only when the barrier reached zero and `HSETNX`
   wins the continuation flag.

A worker can die before the script, and the task redelivers. It can die after
the script, and the acknowledgement is already durable. It cannot die inside
the script.

## Idempotence comes from content-derived keys

Delivery is at least once, so every transition must tolerate replay. The task
key derives from the work itself ([references](store.md)), so the guards above
make a duplicate completion a no-op. The barrier decrements once per task key,
the continuation fires once per job, and the first result written wins.

Retry progress lives in the task record (`:attempt`, `HINCRBY`), not in any
process. The third attempt can run on a worker born after the first two died.
It runs at `now + backoff^attempt` through the `delayed` zset, where an atomic
`ZREM` lets exactly one mover win each due task.

## History is a per-job event stream

The same scripts append each transition to `job:<id>:log`, with the worker id
and a timestamp: claimed, completed, failed, redelivered, and the barrier
counts. That is the execution history Step Functions sells. `fleet:status`
reads the materialized records, audit and debugging replay the log, and
retention is a TTL.

## The ledger makes Redis rebuildable

Before anything is enqueued, the job spec is written as a content-addressed
blob, on the durable store tier where one is configured. The spec holds the env
hash, the entry, the argument references, the policy and the continuation.
Redis is then the control plane, and losing it is an inconvenience rather than
an amputation.

`fleet recover` reads the spec and derives every task key again. Dumps are
deterministic, so the derivation is a pure function. It checks which results
already exist and submits only the gap. Recovery is idempotent, and it is safe
to run beside normal traffic.

Step Functions cannot do this, because an execution is not derivable from its
input: its history is its state. Fleet's state is a function of spec and
results, so its history is advisory and determinism is the recovery substrate.

## Safety and liveness are separate

Safety means no acknowledged transition is lost, and Redis persistence bounds
it. AOF `everysec` loses at most a second, and the ledger bounds even total
loss. Liveness means timers fire, retries redeliver and schedules enqueue, and
that needs a clock somewhere. Fleet has no serviceful interpreter, so
[liveness](liveness.md) owns that design and states its remaining gap.

Fleet provides its own pulse, and in exchange it escapes the limits the service
imposes:

| Step Functions limit | Fleet |
|----------------------|-------|
| 256 KB payloads between states | references are unbounded |
| 25,000-event histories | a log stream with a TTL |
| one-year execution cap | none |
| a price per transition, rivaling the compute bill in slay's own cost model | Redis commands |

## Wrappers compile to policy

Composition still reads as ordinary code. `fleet:retry`, `fleet:pure` and
`fleet:rate-limit` wrap a function value, as slay's wrappers wrapped a
reference. Each wrapper compiles down to policy fields on the records it emits,
never to a loop inside a process. The in-process form exists too, a bare retry
around a sub-call inside one task, and it is durable only as long as the task
around it.

Retry policy also carries `:retry-on`, an error-class filter. A timeout can
retry while a type error dead-letters at once. slay composed the same
distinction from two retrier classes, and fleet folds it into one field.

Branching that Step Functions writes as an ASL `Choice` state is a
continuation that computes and enqueues the next stage: code, not JSON. An
external completer, the task-token pattern, is anyone who holds a task key and
writes its result.

## Memoization and coordination

Memoization is durable by construction. `fleet:pure` reads `result:<task-key>`
before it enqueues and publishes after. A `:salt` folded into the key busts the
cache. `fleet:cache-only`, slay's `everpure`, reads the cache and never
enqueues, and it fails a miss with a named error: read-only access to expensive
results.

The two coordination wrappers keep their state in the substrate, shared by
every worker. `fleet:rate-limit` keeps a token bucket in `rate:<name>`, and
`fleet:semaphore` keeps a counter in `sem:<name>`.
