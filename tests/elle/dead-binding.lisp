(elle/epoch 12)
# ── Dead binding elimination: what it removes, and what it must not ─────
#
# The pass removes an unused let/letrec binding together with its initializer.
# These run the compiled program, so they observe the only thing that matters
# about the transform: the program's behavior is unchanged.

# ── An unused binding over an effect-free initializer ───────────────────

(assert (= (let [unread (%add 1 2)
                 kept 10]
             kept) 10)
        "dropping a dead arithmetic binding leaves the result alone")

(assert (= (let [alias 7
                 unread alias
                 kept alias]
             kept) 7) "dropping a dead alias leaves its source alone")

# ── A silent initializer that mutates ──────────────────────────────────
#
# The trap this pins: %push-array-mut emits no signal, so a rule that keyed on
# silence alone would delete the append. `arr` would then still hold two
# elements and the assertion below would read 2.

(def arr @[1 2])
(let [unread (%push-array-mut arr 3)
      kept 10]
  (assert (= kept 10) "the surrounding let still evaluates"))
(assert (= (length arr) 3)
        "an unused binding does not delete the append its initializer made")

# ── Through a user-defined function ────────────────────────────────────

(defn append-one [a]
  (%push-array-mut a 9))
(def arr2 @[1])
(let [unread (append-one arr2)
      kept 0]
  (assert (= kept 0) "the surrounding let still evaluates"))
(assert (= (length arr2) 2)
        "purity is not inherited from a silent signal one call deep")

# ── An observable initializer ──────────────────────────────────────────

(def @writes 0)
(defn note []
  (assign writes (+ writes 1))
  nil)
(let [unread (note)
      kept 5]
  (assert (= kept 5) "the surrounding let still evaluates"))
(assert (= writes 1) "an initializer that writes still runs")

# ── A used binding is untouched ────────────────────────────────────────

(assert (= (let [x (%add 20 22)]
             x) 42) "a read binding keeps its value")

# ── Recursion still terminates and still computes ──────────────────────
#
# A self-recursive function is never proven pure, so nothing about its call
# sites changes.

(defn total [n acc]
  (if (= n 0) acc (total (- n 1) (+ acc n))))
(assert (= (total 4 0) 10) "recursion is unaffected")

(println "all dead binding tests passed")
