(elle/epoch 12)
# A process scheduler owns no I/O backend: it forwards each request to
# the root scheduler and waits for the completion to come back down.
# This file pins what it may do while it waits.
#
# The invariant: a process that can still run, runs. The process
# scheduler never blocks until a forwarded completion arrives while any
# process is ready, because the completion can depend on that very
# process — an h2 client sub-fiber parked in `read` is waiting for the
# request its own process has not finished sending. See
# `tests/elle/h2-headers-in-process.lisp` for that shape end to end.
#
# The case below makes the dependency impossible to satisfy the other
# way round. The only outstanding I/O is a 30 s sleep, and the process
# has a compute loop to finish that outlasts many fuel quanta. Running
# the process costs about a second; waiting on the sleep costs 30, which
# is past the deadline here.
#
# The `ev/sleep` in the process body is what puts the sleeper's timer in
# flight. Sub-fibers are drained only in a round where some ready process
# is not merely refueling, and a process that runs straight from
# `ev/spawn` into the loop never gives the scheduler such a round.
# Without that sleep there is no outstanding completion and the case
# tests nothing. The assertion that follows holds the ordering.

(def process ((import "std/process")))

(def @sleeper-ran @[false])

(defn sleep-long []
  (put sleeper-ran 0 true)
  (ev/sleep 30))

(defn sum-to [n]
  (def @acc 0)
  (each i in (range 0 n)
    (assign acc (+ acc i)))
  acc)

(defn body []
  (let [sleeper (ev/spawn sleep-long)]
    (ev/sleep 0.01)
    (assert (get sleeper-ran 0) "the sleeper's I/O is in flight before the loop")
    (assert (= (sum-to 20000) 199990000) "the loop ran to completion")
    (ev/abort sleeper)))

(def result
  (ev/timeout 20
              (fn []
                (process:start body)
                :done)))

(assert (= result :done)
        "a ready process runs while a forwarded completion is outstanding")
