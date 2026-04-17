# ── Loop agreement across tiers ─────────────────────────────────────
#
# Covers: while loops (Jump back-edges, Branch terminators),
# local variables (StoreLocal/LoadLocal across blocks), nested loops,
# and float accumulation in loops.

(def diff ((import "tests/diff/harness")))

# ── Sum 0..n ─────────────────────────────────────────────────────

(defn sum-to [n]
  (var s 0)
  (var i 0)
  (while (< i n)
    (assign s (+ s i))
    (assign i (+ i 1)))
  s)

(diff:assert-agree sum-to 0)
(diff:assert-agree sum-to 1)
(diff:assert-agree sum-to 10)
(diff:assert-agree sum-to 100)

# ── Factorial ────────────────────────────────────────────────────

(defn factorial [n]
  (var p 1)
  (var i 1)
  (while (<= i n)
    (assign p (* p i))
    (assign i (+ i 1)))
  p)

(diff:assert-agree factorial 0)
(diff:assert-agree factorial 1)
(diff:assert-agree factorial 5)
(diff:assert-agree factorial 10)

# ── Fibonacci ────────────────────────────────────────────────────

(defn fib [n]
  (var a 0)
  (var b 1)
  (var i 0)
  (while (< i n)
    (let [[t b]]
      (assign b (+ a b))
      (assign a t))
    (assign i (+ i 1)))
  a)

(diff:assert-agree fib 0)
(diff:assert-agree fib 1)
(diff:assert-agree fib 5)
(diff:assert-agree fib 10)
(diff:assert-agree fib 20)

# ── Nested loop: n×n counter ────────────────────────────────────

(defn grid [n]
  (var s 0)
  (var i 0)
  (while (< i n)
    (var j 0)
    (while (< j n)
      (assign s (+ s 1))
      (assign j (+ j 1)))
    (assign i (+ i 1)))
  s)

(diff:assert-agree grid 0)
(diff:assert-agree grid 1)
(diff:assert-agree grid 5)
(diff:assert-agree grid 10)

# ── Float accumulation in loop ───────────────────────────────────

(defn float-accum [n]
  (var s 0.0)
  (var i 0)
  (while (< i n)
    (assign s (+ s 1.5))
    (assign i (+ i 1)))
  s)

(diff:assert-agree float-accum 0)
(diff:assert-agree float-accum 1)
(diff:assert-agree float-accum 4)
(diff:assert-agree float-accum 10)

(println "loop: OK")
