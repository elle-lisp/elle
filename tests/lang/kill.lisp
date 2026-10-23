(elle/epoch 15)
# audited: 2026-10-22
# sys/kill ends a worker at its next charge, and a join on it raises :killed.
# docs/threads.md
#
# The counter-factual: a kill that only a metered fiber reads leaves an
# unmetered worker spinning, and its join never returns. A kill raised as a
# plain error is caught by the worker's protect, and the join answers :caught.

(defn spin-forever []
  "A silent loop with no end: proven operands, no call, nothing that raises."
  (def @i 0)
  (while true (assign i (%add i 1)))
  i)

## A kill lands inside a silent loop, in a worker with no budget.
(let [h (sys/spawn-vm spin-forever)]
  (sys/kill h)
  (let [[ok? err] (protect (sys/join h))]
    (assert (not ok?) "silent: a killed worker answers no value")
    (assert (= (get err :error) :killed) "silent: the join raises :killed")))

## A kill lands inside a loop that may raise.
(let [h (sys/spawn (fn []
                     (def @i 0)
                     (while true (assign i (+ i 1)))
                     i))]
  (sys/kill h)
  (let [[ok? err] (protect (sys/join h))]
    (assert (= (get err :error) :killed) "raising: the join raises :killed")))

## No cleanup body runs: the kill passes protect and defer.
(let [h (sys/spawn-vm (fn []
                        (let [[ok? e] (protect (defer
                                (spin-forever)
                                (spin-forever)))]
                          :caught)))]
  (sys/kill h)
  (let [[ok? err] (protect (sys/join h))]
    (assert (= (get err :error) :killed)
            "protect: the worker did not catch the kill")))

## The thread's state names the kill.
(let [h (sys/spawn-vm spin-forever)]
  (sys/kill h)
  (protect (sys/join h))
  (assert (= (sys/thread-state h) [:killed])
          "state: thread-state answers [:killed]"))

## A kill after the end changes nothing, and a second kill is harmless.
(let [h (sys/spawn-vm (fn [] 7))]
  (assert (= 7 (sys/join h)) "after: the worker finished")
  (sys/kill h)
  (sys/kill h)
  (assert (= 7 (sys/join h)) "after: a kill after the end leaves the result"))

## Only a thread handle can be killed.
(let [[ok? err] (protect (sys/kill 42))]
  (assert (not ok?) "type: a number is not a worker")
  (assert (= (get err :error) :type-error)
          "type: sys/kill refuses it as a type error"))

## A bounded join with :kill true kills the worker when the bound passes.
(let [h (sys/spawn-vm spin-forever)]
  (let [[ok? err] (protect (sys/join h :timeout 0.05 :kill true))]
    (assert (not ok?) "kill-join: the bound passes")
    (assert (= (get err :error) :timeout)
            "kill-join: and the join raises :timeout"))
  (let [[ok? err] (protect (sys/join h))]
    (assert (= (get err :error) :killed) "kill-join: the kill landed")))

## A deadline kills as a timeout does.
(let [h (sys/spawn-vm spin-forever)]
  (let [[ok? err] (protect (sys/join h :deadline (+ (clock/monotonic) 0.05)
                                     :kill true))]
    (assert (= (get err :error) :timeout) "deadline: the join raises :timeout"))
  (let [[ok? err] (protect (sys/join h))]
    (assert (= (get err :error) :killed) "deadline: the kill landed")))

## :kill true needs a bound.
(let [h (sys/spawn-vm (fn [] 1))]
  (let [[ok? err] (protect (sys/join h :kill true))]
    (assert (not ok?) "unbounded: a kill with no bound is refused")
    (assert (= (get err :error) :argument-error)
            "unbounded: as an argument error"))
  (assert (= 1 (sys/join h)) "unbounded: and the worker is untouched"))

## A worker that finishes inside the bound is not killed.
(let [h (sys/spawn-vm (fn [] 3))]
  (assert (= 3 (sys/join h :timeout 5 :kill true))
          "in-time: a worker that finishes in time keeps its value"))
