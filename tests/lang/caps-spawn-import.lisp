(elle/epoch 16)
# audited: 2026-11-13
# The scheduler refuses an import from a spawned fiber denied :import, and loads for one denied :fs on a fiber denied :fs too.
# docs/signals/capabilities.md
# docs/modules.md

(include-file "caps/spawn.lisp")

# An import emits :import, which the scheduler answers as a request. A denial
# carries the bit too, and the scheduler refuses it rather than load a module
# for a fiber that may not import.
(assert (refused? (spawned |:import| (fn [] (protect (import "std/base64"))))
                  "fiber/emit" :import) "a denied import is refused")

# The scheduler loads a module on a fiber that withholds what the importer
# withholds. The counter-factual loaded with the scheduler's own capabilities,
# and a fiber denied :fs read a file through its import.
(let [[ok? denial] (spawned |:fs| (fn [] (protect (import "../modules/test"))))]
  (assert (and (not ok?) (= :capability-denied (get denial :error))
               (contains? (get denial :denied) :fs))
          "a load for a fiber denied :fs is refused"))

(println "caps-spawn-import: OK")
