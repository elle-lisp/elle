# Fleet: the queue

<!-- audited: 2026-10-22 -->

The Redis keys a fleet coordinates through, the task lifecycle over them, and
the delivery, flow-control and ordering policies.

[Orchestration](orchestration.md) owns the transition scripts that write these
keys. Nothing here is implemented.

## The substrate contract is the protocol

Coordination lives in Redis, through `std/redis`, which speaks RESP2; streams
are ordinary commands. The contract is exactly that protocol: RESP2, `EVAL`,
streams, zsets and pub/sub. Any Redis-compatible service that speaks them
qualifies, whether self-hosted, Sentinel or managed. Fleet never assumes it can
run code on or beside the server.

A cluster-mode service shards by key slot, and the transition scripts are
multi-key. So every control-plane key shares one hash tag, `{fleet}`, and lands
in one slot, while blobs stay untagged and spread. The queue and the store sit
behind small interfaces, and the local backend is the second implementation
that keeps them honest ([the client](api.md)).

| Key (prefix `fleet:`) | Type | Content |
|---|---|---|
| `blob:<hash>` | string | encoded bytes |
| `tasks:<fingerprint>` | stream | the tasks for one binary layout, group `workers` |
| `tasks:<fingerprint>:<pool>` | stream | tasks bound to a named worker pool || `lane:<pipeline>:<n>` | stream | one FIFO lane of an ordered pipeline |
| `queue:<name>` | hash | named-queue control: the paused flag and the caps |
| `task:<key>` | hash | policy, status, attempt, timestamps, worker id |
| `job:<id>` | hash | the task-key list, the remaining counter, the continuation flag |
| `job:<id>:done` | set | the task keys the barrier has already counted |
| `job:<id>:log` | stream | the transition history, the durable audit trail |
| `delayed` | zset | task keys due for redelivery, scored by time |
| `dead` | list | tasks past their retry budget, with error records |
| `result:<key>` | string | a result reference or an error record |
| `done` | pub/sub | task and job completion notices |
| `worker:<id>` | hash with a TTL | fingerprint, host, capacity, heartbeat |
| `sem:<name>`, `rate:<name>` | — | a distributed semaphore, a token bucket |

## Lifecycle

A client submits, a worker claims with `XREADGROUP`, and the worker completes
or fails. Each submit, complete, fail and redeliver step is one atomic
transition script, so no crash leaves the records torn.

A worker that dies mid-task stops heartbeating, and its peers reclaim its
pending entries with `XAUTOCLAIM` after an idle deadline. A task whose attempt
count exceeds its budget moves to `dead` with its error record, so nothing is
dropped silently. `dead` is not a grave: `fleet:redrive` resubmits a dead task
with a fresh budget.

## Delivery

Delivery defaults to **at-least-once**. A pure task composes that with
memoization into effectively-once. A side-effecting task gets at-least-once,
and the design says so.

`:at-most-once` names the opposite policy, which slay offered for its fire-and-
forget call. It sets no retry budget, and a reclaim routes the task to `dead` as
*lost* instead of redelivering it. A task such as "send the email" then runs
once or is recorded lost; it never runs twice.

## Flow control

A queue carries flow control. `fleet:pause` sets a flag the claim loop honors,
`fleet:resume` clears it, and `fleet:purge` drains a queue's pending and
delayed entries in one script. All three are ordinary transitions, recorded in
the log like any other.

## Ordered execution

Ordered execution is a **pipeline**. `fleet:pipeline` stripes its pushes
round-robin across `n` FIFO lanes, and each lane permits one delivery in flight.
Order holds within a lane, and the lanes run in parallel.

FIFO has a price. A failing task at a lane's head blocks that lane through its
retry budget, then dead-letters, and the lane moves on.
