(elle/epoch 16)
# audited: 2026-11-09
# The link to the parent scheduler: forwards I/O up, and parks on a futex key of this scheduler's own.
# lib/process/overview.md
# docs/process-scheduler.md
#
# The key is taken at the first forward. A process system that forwards no I/O
# then reads no process state, and runs to its end in a fiber denied :ambient.

(fn []
  (def completions @[])  # completions forwarded by the parent scheduler
  (def wake-box (box 0))  # futex box the parent reboxes to wake us
  (def @wake-key nil)  # the futex key we park on in the parent

  (defn key []
    "The futex key we park on in the parent, taken on the first call."
    (when (nil? wake-key) (assign wake-key (sys/unique)))
    wake-key)

  (defn forward [request]
    "Hand an I/O request to the parent scheduler. Returns its id or [:error e]."
    (emit :wait {:op :io-forward
                 :request request
                 :queue completions
                 :wake-box wake-box
                 :wake-key (key)}))

  (defn park [expected]
    "Park in the parent until it reboxes the wake-box away from expected."
    (ev/futex-wait (key) wake-box expected))

  {:completions completions :wake-box wake-box :forward forward :park park})
