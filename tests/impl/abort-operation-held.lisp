(elle/epoch 18)
# audited: 2026-11-20
# An aborted fiber's operation stays paired with it until it answers, and its cleanup finds the operation ended.
# docs/scheduler.md
#
# A fiber sleeps, its cleanup records what `ev/report` says, and the case
# aborts it. On the thread pool a sleep holds a worker, so the cleanup can
# see whether the sleep's cancel has answered; on the ring the worker count
# stays flat and only the pairing says anything.
#
# The counter-factual is an abort that drops the pairing and raises at once:
# `:io` falls at the abort, and the cleanup runs while the pool's worker is
# still out.

(defn io-count []
  (get (ev/report) :io))
(defn worker-count []
  (get (ev/report) :workers))

(let* [io-base (io-count)
       worker-base (worker-count)
       seen @[]
       f (ev/spawn (fn []
                     (defer
                       (push seen [(io-count) (worker-count)])
                       (ev/sleep 30))))]
  (while (= (io-count) io-base) (ev/sleep 0.001))
  (ev/abort f)
  (assert (= (+ io-base 1) (io-count))
          "the aborted fiber's operation stays paired with it until it answers")
  (assert (= [false {:error :aborted}] (ev/join-protected f))
          "the fiber ends on the abort's error")
  (assert (= io-base (io-count)) "the pairing went with the answer")
  (assert (= [[io-base worker-base]] (freeze seen))
          (string "the cleanup found the operation ended: " (freeze seen)
                  " rather than " [[io-base worker-base]])))

(println "tests/impl/abort-operation-held.lisp: all tests passed")
