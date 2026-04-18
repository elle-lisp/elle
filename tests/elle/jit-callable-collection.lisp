(elle/epoch 8)
# JIT callable collection dispatch
#
# Regression test: the JIT call/tail-call paths lacked call_collection
# dispatch, so callable collections (@array, struct, @struct, etc.)
# produced "Cannot call" errors when the calling closure was JIT-compiled.
# This test forces JIT compilation by looping enough to trigger tiering.

# ── 1. @array callable through JIT ──────────────────────────────────

(let [arr @[10 20 30]]
  (defn get-idx [i] (arr i))
  # Loop enough to trigger JIT compilation
  (repeat 200 (get-idx 1))
  (assert (= 20 (get-idx 1)) "1a: @array callable after JIT"))
(println "1: @array callable ok")

# ── 2. struct callable through JIT ──────────────────────────────────

(let [s {:a 1 :b 2 :c 3}]
  (defn get-key [k] (s k))
  (repeat 200 (get-key :b))
  (assert (= 2 (get-key :b)) "2a: struct callable after JIT"))
(println "2: struct callable ok")

# ── 3. @array tail-call through JIT ─────────────────────────────────

(let [arr @[100 200 300]]
  (defn tail-get [i] (arr i))
  (repeat 200 (tail-get 2))
  (assert (= 300 (tail-get 2)) "3a: @array tail-call after JIT"))
(println "3: @array tail-call ok")

# ── 4. struct tail-call through JIT ─────────────────────────────────

(let [s {:x 42}]
  (defn tail-lookup [] (s :x))
  (repeat 200 (tail-lookup))
  (assert (= 42 (tail-lookup)) "4a: struct tail-call after JIT"))
(println "4: struct tail-call ok")

# ── 5. string callable through JIT ──────────────────────────────────

(let [text "hello"]
  (defn char-at [i] (text i))
  (repeat 200 (char-at 0))
  (assert (= "h" (char-at 0)) "5a: string callable after JIT"))
(println "5: string callable ok")

(println "all jit-callable-collection tests passed")
