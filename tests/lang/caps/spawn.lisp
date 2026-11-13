(elle/epoch 16)
# audited: 2026-11-13
# What every caps-spawn test shares: a sandbox denied a set, a spawn joined inside it, and the shape of a refusal.
# docs/signals/capabilities.md
# tests/lang/overview.md
#
# The sandbox's own mask names only :error. A denial that escapes the scheduler
# therefore reaches the root as an unhandled signal and fails the file, rather
# than landing quietly in a parent that happens to catch it.

(defn sandboxed [deny thunk]
  "Run thunk in a fiber denied `deny`, and return what it returns."
  (let [f (fiber/new thunk |:error| :deny deny)
        v (fiber/resume f)]
    (assert (= (fiber/status f) :dead) "the sandbox runs to completion")
    v))

(defn spawned [deny thunk]
  "Spawn thunk from inside a sandbox denied `deny`, and join it there."
  (sandboxed deny (fn [] (ev/join (ev/spawn thunk)))))

(defn refused? [outcome primitive bit]
  "Whether a protect outcome is the refusal of `primitive`, denied `bit`."
  (let [[ok? denial] outcome]
    (and (not ok?) (= (get denial :error) :capability-denied)
         (= (get denial :primitive) primitive)
         (contains? (get denial :denied) bit))))
