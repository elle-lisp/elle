(elle/epoch 13)
# audited: 2026-09-29
# A finished fiber answers every join, however many arrive and however late.
# docs/scheduler.md
#
# The status and the value of a fiber live in the fiber, so a second join
# answers exactly as the first did, a protected join reports a failure every
# time it is asked, and a join that arrives after the fiber ran unjoined still
# reads its value. A scheduler is free to forget what it remembered about a
# finished fiber; it is never free to change the answer.
#
# The counter-factual: a scheduler that answered joins from a record it keeps
# beside the fiber, and dropped that record at the first join or at
# completion, answers the second or the late join with nil.

(println "a retired record is re-derivable...")

(let [f (ev/spawn (fn [] 42))]
  (assert (= (ev/join f) 42) "the first join returns the fiber's value")
  (assert (= (ev/join f) 42) "a second join returns it again, from the fiber")
  (assert (= (fiber/status f) :dead) "the fiber itself still reads terminal"))

(let [g (ev/spawn (fn [] (error {:error :boom})))]
  (let [[ok? val] (ev/join-protected g)]
    (assert (not ok?) "the protected join reports the failure")
    (assert (= (get val :error) :boom) "and carries the error value"))
  (let [[ok2? val2] (ev/join-protected g)]
    (assert (not ok2?) "a second protected join reports it again")
    (assert (= (get val2 :error) :boom) "with the same error value")))

(println "a retired unjoined fiber still answers a later join...")

(let [f (ev/spawn (fn [] 42))]
  (ev/sleep 0)
  (assert (= (fiber/status f) :dead) "the fiber ran without being joined")
  (assert (= (ev/join f) 42) "and a later join reads its value from itself"))

(println "ev-join-repeat: ok")
