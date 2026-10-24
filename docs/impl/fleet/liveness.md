# Fleet: liveness and the tick

<!-- audited: 2026-10-21 -->

The three fleet duties that need a clock, the maintenance pass that does them,
and the processes that run it.

[Orchestration](orchestration.md) owns the transition scripts the tick is built
from, and [the queue](queue.md) owns the keys it reads. Nothing here is
implemented.

## Three duties need a clock

Exactly three duties in a fleet need a clock, and nothing else does:

- **redelivery**: move due entries from `delayed` into their stream;
- **reclaim**: `XAUTOCLAIM` the deliveries idle past their deadline;
- **schedules**: enqueue the due `run-at` and recurring tasks.

Everything else runs without a pulse. A heartbeat expires as a Redis TTL, a
barrier clears on a task completion, and a client wakes on pub/sub with a poll
fallback. So the liveness gap is narrow: **Redis stores the timers but cannot
run a script on a clock**.

## The tick

Fleet folds the three duties into **the tick**: one maintenance pass built from
the same atomic transition scripts. The tick is idempotent and race-safe,
because `ZREM` and `XAUTOCLAIM` let exactly one ticker win each due item. So any
number of processes may tick, at any frequency, forever. Correctness never
depends on who ticks; only lateness does.

**The tick lives server-side, as Redis scripts any RESP client can invoke.** No
Elle runs in the invoker, and it holds no fleet code and no state: it connects,
calls `EVALSHA`, and disconnects. The scripts judge due-ness against the Redis
server's own clock (`TIME` inside the script), so an invoker with a skewed clock
cannot fire a timer early. That constraint is what makes the pulse a commodity.

`fleet timerd` is the resident invoker. Anything with a clock and a Redis
connection is an equally valid one:

| Pulse source | Ticks | Worst-case timer lateness |
|---|---|---|
| workers | between claims | seconds, on any live fleet |
| waiting clients | while blocked in `run` or `attach` | the same, while anyone waits |
| `fleet timerd` | every second, on your machines | about 1 s, even on an idle fleet |
| a managed scheduler calling a thin function | on its cron | about 1 min, or 1 s sustained |
| host cron running `fleet tick` | every minute | about 1 min, if timerd dies |
| the next participant | on connect | unbounded, and exact |

The first two rows make a busy fleet pulse itself. The dedicated pulse depends
on where the control plane lives.

## Self-hosted Redis

Run timerd where Redis runs, under the same supervisor. It inherits the control
plane's availability and adds no new way for the fleet to be down: a dark host
has no control plane to act on, so it has no liveness to provide.

A Sentinel deployment runs one timerd beside each replica, and a lease in Redis
(`SET NX PX`, renewed) elects the ticker. A broken lease is harmless, because
concurrent ticking wastes work and cannot deliver twice.

## Managed Redis

A managed service offers only the protocol, and nothing runs beside it, so the
clock is rented too. A managed scheduler invokes a stateless function that
speaks RESP and runs the tick: EventBridge Scheduler, Cloud Scheduler, a
Workers cron trigger or a Kubernetes CronJob. State durability then comes from
the Redis provider's replication and the pulse from the scheduler's SLA. Both
are multi-AZ, and none of the machines are yours.

A scheduler's floor is typically one minute. An invocation that ticks each
second for its minute buys second-level lateness for an idle fleet, and a busy
fleet pulses itself anyway.

The scheduler is a cloud dependency, and a thin one: a pulse, not an
interpreter, with no logic, no state and no payload. Every transition still
lives in the scripts. A dead scheduler loses nothing, and timers fire late until
another pulse appears. Changing providers means rewriting a twenty-line
function. slay's second system adopted Lambda, S3 and Step Functions as its
runtime, and fleet at most rents a clock.

Durability and serverless compute are not what Step Functions keeps over this
shape. State and pulse both carry provider SLAs here, and an ephemeral worker
that the pulse starts on backlog closes the compute gap ([workers](workers.md)).
What remains is single-vendor integration: one console, one IAM boundary, one
bill.

## Corollaries

Scheduled execution is trustworthy only with a dedicated pulse. So the tick
scripts and timerd land with the queue milestone, well before the schedule
milestone that leans on them ([the plan](plan.md)).

A cold fleet, with every pulse dead, loses no state and re-arms completely on
the next connect. The first tick drains everything due, in order, exactly once.
