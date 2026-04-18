(elle/epoch 8)
# ── compile/parallelize tests ─────────────────────────────────────────

# ── Safe: independent functions ──────────────────────────────────────

(def src "
(defn fetch-a [] (+ 1 2))
(defn fetch-b [] (+ 3 4))
(defn fetch-c [] (+ 5 6))
")
(def a (compile/analyze src))

(def result (compile/parallelize a [:fetch-a :fetch-b :fetch-c]))
(assert (get result :safe) "independent functions are safe to parallelize")
(assert (not (nil? (get result :reason))) "reason is provided")
(assert (not (nil? (get result :code))) "generated code is provided")
(assert (not (nil? (get result :signal))) "signal is computed")

# ── Unsafe: shared mutable capture ───────────────────────────────────

(def src2 "
(var state 0)
(defn update-counter [] (assign state (+ state 1)))
(defn update-state [] (assign state (* state 2)))
")
(def a2 (compile/analyze src2))

(def r2 (compile/parallelize a2 [:update-counter :update-state]))
(assert (not (get r2 :safe)) "shared mutable capture is unsafe")
(assert (not (nil? (get r2 :reason))) "unsafe reason is provided")
(assert (not (nil? (get r2 :shared-captures))) "shared captures are listed")

(def shared (get r2 :shared-captures))
(assert (not (empty? shared)) "shared captures is non-empty")
(def first-shared (first shared))
(assert (= (get first-shared :name) "state") "shared capture is 'state'")

# ── Mixed: some safe pairs ──────────────────────────────────────────

(def src3 "
(defn pure-a [] 1)
(defn pure-b [] 2)
")
(def a3 (compile/analyze src3))
(def r3 (compile/parallelize a3 [:pure-a :pure-b]))
(assert (get r3 :safe) "pure functions are safe")
(assert (get (get r3 :signal) :silent) "pure functions have silent signal")

# ── Signal combination ───────────────────────────────────────────────

(def src4 "
(defn reader [] (println \"hello\"))
(defn pure [] 42)
")
(def a4 (compile/analyze src4))
(def r4 (compile/parallelize a4 [:reader :pure]))
(assert (get r4 :safe) "no shared captures = safe")
# Combined signal should include io from reader
(def combined-sig (get r4 :signal))
(assert (not (get combined-sig :silent)) "combined signal is not silent")

(println "all compile/parallelize tests passed")
