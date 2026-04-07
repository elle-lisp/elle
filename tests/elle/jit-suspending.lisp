# JIT SuspendingCall + polymorphic signal tests
#
# Regression tests for JIT compilation of:
# 1. SuspendingCall — calls to functions that may yield
# 2. Polymorphic functions — functions that call parameters as functions
#    (struct/array/string callable dispatch)
#
# These were previously rejected by the JIT. The yield-through-call
# side-exit and elle_jit_call runtime dispatch now handle them.

# ── 1. Polymorphic: call struct as function via parameter ────────────

(defn lookup [s k] (s k))
(def data {:x 42 :y 99 :z 7})

# Hot loop to trigger JIT
(var i 0)
(while (< i 200)
  (lookup data :x)
  (assign i (+ i 1)))

(assert (= 42 (lookup data :x)) "1a: struct call via JIT param")
(assert (= 99 (lookup data :y)) "1b: struct call different key")
(assert (= 7  (lookup data :z)) "1c: struct call third key")
(println "1: polymorphic struct call ok")

# ── 2. Polymorphic: call array as function via parameter ─────────────

(defn nth [arr idx] (arr idx))
(def nums @[10 20 30 40 50])

(var i 0)
(while (< i 200)
  (nth nums 2)
  (assign i (+ i 1)))

(assert (= 10 (nth nums 0)) "2a: array call idx 0")
(assert (= 30 (nth nums 2)) "2b: array call idx 2")
(assert (= 50 (nth nums 4)) "2c: array call idx 4")
(println "2: polymorphic array call ok")

# ── 3. Polymorphic: call string as function via parameter ────────────

(defn char-at [s idx] (s idx))
(def text "hello")

(var i 0)
(while (< i 200)
  (char-at text 0)
  (assign i (+ i 1)))

(assert (= "h" (char-at text 0)) "3a: string call idx 0")
(assert (= "o" (char-at text 4)) "3b: string call idx 4")
(println "3: polymorphic string call ok")

# ── 4. Pure computation in hot loop (no SuspendingCall) ──────────────

(defn sum-to [n]
  (var sum 0)
  (var i 0)
  (while (< i n)
    (assign sum (+ sum i))
    (assign i (+ i 1)))
  sum)

(var i 0)
(while (< i 200)
  (sum-to 10)
  (assign i (+ i 1)))

(assert (= 4950 (sum-to 100)) "4a: hot loop result correct")
(assert (= 0    (sum-to 0))   "4b: edge case n=0")
(assert (= 1    (sum-to 2))   "4c: small n")
(println "4: pure computation ok")

# ── 5. Mixed: polymorphic + computation in hot loop ──────────────────
#
# Simulates the Game of Life pattern: a function that reads from an
# array parameter in a tight loop.

(defn sum-array [arr n]
  (var sum 0)
  (var i 0)
  (while (< i n)
    (assign sum (+ sum (arr i)))
    (assign i (+ i 1)))
  sum)

(def test-arr @[1 2 3 4 5 6 7 8 9 10])

(var i 0)
(while (< i 200)
  (sum-array test-arr 10)
  (assign i (+ i 1)))

(assert (= 55 (sum-array test-arr 10)) "5a: sum-array full")
(assert (= 15 (sum-array test-arr 5))  "5b: sum-array partial")
(assert (= 0  (sum-array test-arr 0))  "5c: sum-array empty")
(println "5: polymorphic hot loop ok")

# ── 6. Verify no spurious rejections ─────────────────────────────────
#
# After JIT, the functions above should NOT appear in jit/rejections.

(def rejections (jit/rejections))
(def rejected-names (map (fn [r] (get r :name)) rejections))

(defn name-rejected? [name]
  (var found false)
  (each r rejections
    (when (= (get r :name) name)
      (assign found true)))
  found)

# These should have been JIT-compiled, not rejected
(assert (not (name-rejected? "lookup"))    "6a: lookup not rejected")
(assert (not (name-rejected? "nth"))       "6b: nth not rejected")
(assert (not (name-rejected? "char-at"))   "6c: char-at not rejected")
(assert (not (name-rejected? "sum-array")) "6d: sum-array not rejected")
(assert (not (name-rejected? "sum-to"))    "6e: sum-to not rejected")
(println "6: no spurious rejections ok")

(println "all jit-suspending tests passed")
