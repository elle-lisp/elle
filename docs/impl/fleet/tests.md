# Fleet: the tests

<!-- audited: 2026-10-21 -->

The pins each fleet milestone must land with, one entry per subject.

[The plan](plan.md) owns which milestone each subject lands in. Nothing here is
implemented.

## Data

- **Store.** Every `SendValue`-encodable type round-trips through the blob
  store. The inline tier and the store tier cut over at the size boundary.
  Identical values collapse to one blob. Counter-factual: two distinct values
  never share a hash entry.
- **Vacated blobs.** Expire a result blob and fetch it: the error is
  `:vacated`, naming the hash. A client holding the value uploads it again, and
  the fetch succeeds. A vacated `fleet:pure` result recomputes. Flip one byte in
  a stored blob, and the fetch fails `:corrupt` before any decode.
- **Identity.** One session dumped twice yields one env hash, and a changed
  binding changes it. `fleet:pure` executes once for identical submissions,
  pinned by a side-effect counter that would read 2 under re-execution.

## The queue

- **Lifecycle.** A task submitted before any worker exists completes when one
  arrives. A killed worker's claimed task is reclaimed and finishes elsewhere. A
  task that fails past its retry budget lands in dead-letter with its error
  record, exactly once.
- **Durable retry.** Kill the worker between attempt one and attempt two. A
  fresh worker honors the remaining budget and the recorded backoff, because
  the attempt counter lives in the substrate.
- **At-most-once.** Kill a worker mid-task under `:at-most-once`. The reclaim
  routes the task to `dead` as lost, never back to the stream. A side-effect
  counter reads at most one, where redelivery would read two.
- **Flow control.** `fleet:pause` stops claims while submissions still enqueue.
  `fleet:resume` drains the backlog. `fleet:purge` empties the pending and
  delayed entries, and the log records all three.
- **Pipelines.** With width two and a failing task at one lane's head, that
  lane holds order behind the retries and dead-letters the head while the other
  lane proceeds.

## Jobs and transitions

- **`:continue`.** A job with one failing task among a hundred completes the
  other ninety-nine. `fleet:results` yields them, `fleet:errors` yields the one
  record, and `fleet:gather` raises only after every task reached a terminal
  state.
- **`:cancel`.** The first failure cancels the job. Pending tasks never run, so
  a side-effect counter holds at its pre-cancel count. Running tasks die like
  deadline overruns, completed results stay fetchable, and the cancel survives
  the client that set it. A dead task re-driven with `fleet:redrive` completes
  with a fresh budget.
- **Jobs.** Kill the client mid-`fleet:map`, and `fleet:attach` from a fresh
  process collects the full ordered results. Two workers that complete a job's
  last two tasks at once fire the continuation exactly once, pinned against the
  atomic decrement.
- **Atomicity.** A reclaimed task completed by both its old and its new worker
  decrements the barrier once and records one result. The counter-factual is
  the unguarded script, which reads a remaining count of −1. N concurrent
  tickers draining one due entry redeliver it once. The job log replays to the
  terminal state the records show.
- **Recovery.** Flush Redis after a job completes against a durable store tier.
  `fleet recover` finds every result present and runs nothing again, pinned by a
  side-effect counter. Flush mid-job, and recovery submits only the tasks with
  no result, and the job completes.
- **Durability.** Submit, kill the client, and fetch the result by task key
  from a fresh process.

## The tick

- **Liveness.** With zero workers, a timerd alone redelivers a due retry and
  enqueues a due schedule, both visible in the records, and the work completes
  when a worker joins. Kill the timerd holding the lease: a standby's next tick
  takes the lease, and the pending timer fires once. Stop every pulse with
  timers due and start one worker; its first tick drains all of them, each
  exactly once.
- **The tick over bare RESP.** Drive the tick over a bare RESP connection, with
  raw commands and no `std/fleet` in the invoker, and assert the transitions a
  worker's tick produces. This pin keeps tick logic out of Elle code, where a
  managed-scheduler invoker could not reach it. Due-ness uses the server's
  clock: an invoker with a skewed clock fires nothing early.
- **Elastic.** A `--drain` worker exits on an empty queue. New backlog trips
  the invoker hook and one worker starts, pinned by driving the hook with a
  local command and no cloud.

## Workers

- **Task lifecycle.** A task past its deadline is killed in-process and
  recorded `:timeout`. A busy-loop task that never yields pins it, so the pin
  proves fuel preemption delivers the kill. A task error surfaces as an error
  result and the worker survives. Withheld capabilities deny inside the task.
- **Unload hygiene.** Run a task and unload it, and the worker's region count
  returns to its baseline. Two tasks with different images run in sequence stay
  independent. A result survives its image's unload, because it was encoded
  first.
- **Adhoc.** Define a function in one process, and `fleet:run` it in a worker
  started with no knowledge of it. A lambda that captures a local value
  produces the captured value remotely.
- **Routing.** A worker with a mismatched fingerprint never claims an image
  task. The task completes when a compatible worker joins, and `fleet:status`
  names the incompatibility while it waits. A module-entry task runs on both
  populations. A task naming pool `:gpu` is claimed only by a worker serving
  that pool, and `fleet:status` names an unserved pool.
- **Recursion.** A task that calls `fleet:map` completes on a single-worker
  fleet without deadlock, because the claim loop never blocks on the child
  job's barrier.

## The client

- **Scatter.** `(map f (fleet:scatter items))` gathers to the value
  `(map f items)` computes. A chained map-map realizes through one continuation
  chain, pinned by asserting that the client never fetches the intermediate
  stage's results. Plain `map` over an ordinary collection in a connected
  session touches neither store nor queue: the counter-factual for any
  ambient-dispatch design. [Traits](../../traits.md) already pins the stdlib
  trait arm itself, with a collection whose `:map` method returns a sentinel.
- **Local backend.** The whole list above, minus reclaim and routing, runs
  against `:local` with no Redis present.
