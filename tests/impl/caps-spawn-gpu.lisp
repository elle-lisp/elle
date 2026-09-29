(elle/epoch 13)
# audited: 2026-09-29
# A fiber `ev/spawn` creates in a sandbox denied :gpu is refused its SPIR-V compile, not resumed with nil.
# docs/signals/capabilities.md
#
# tests/lang/caps-spawn.lisp pins the same refusal for each capability every
# implementation carries. `git`, the call :gpu gates here, compiles a closure
# to SPIR-V, an extension of this implementation. The gate runs before the
# compile, so no GPU need exist.
#
# The counter-factual: a spawned fiber whose mask does not name :gpu lets the
# denial propagate past the scheduler, and the sandbox's :error-only mask then
# fails the file at the root.

(defn spawned [deny thunk]
  "Spawn thunk from inside a sandbox denied `deny`, join it there, and return
   what the join answers."
  (let [f (fiber/new (fn [] (ev/join (ev/spawn thunk))) |:error| :deny deny)
        v (fiber/resume f)]
    (assert (= (fiber/status f) :dead) "the sandbox runs to completion")
    v))

(let [[ok? denial] (spawned |:gpu| (fn [] (protect (git (fn [x] x)))))]
  (assert (not ok?) "a denied SPIR-V compile is refused")
  (assert (= (get denial :error) :capability-denied)
          "the refusal is a capability denial")
  (assert (= (get denial :primitive) "git") "naming the compile")
  (assert (contains? (get denial :denied) :gpu) "denied :gpu"))
