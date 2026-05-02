(elle/epoch 9)
## array-keys — immutable arrays as struct/map keys

# ── Basic put/get with array key ─────────────────────────────────────
(let [m @{}]
  (put m [1 2] "x")
  (assert (= (get m [1 2]) "x") "get with array key"))

# ── Immutable struct with array key ──────────────────────────────────
(let [s (struct [1 2] "a" [3 4] "b")]
  (assert (= (get s [1 2]) "a") "struct with array key")
  (assert (= (get s [3 4]) "b") "struct with array key 2"))

# ── has? with array key ─────────────────────────────────────────────
(let [m @{[1 2] "x"}]
  (assert (has? m [1 2]) "has? with array key")
  (assert (not (has? m [1 3])) "has? negative"))

# ── del with array key ──────────────────────────────────────────────
(let [m @{[1 2] "x" :a 1}]
  (del m [1 2])
  (assert (not (has? m [1 2])) "del with array key")
  (assert (has? m :a) "del preserves other keys"))

# ── Nested array keys ───────────────────────────────────────────────
(let [m @{}]
  (put m [[1 2] [3 4]] "nested")
  (assert (= (get m [[1 2] [3 4]]) "nested") "nested array key"))

# ── Array keys with mixed element types ─────────────────────────────
(let [m @{}]
  (put m [1 "two" :three] "mixed")
  (assert (= (get m [1 "two" :three]) "mixed") "mixed element array key"))

# ── Equal arrays hash to same key ───────────────────────────────────
(let [m @{}]
  (put m [1 2 3] "first")
  (assert (= (get m [1 2 3]) "first") "equal arrays same key"))

# ── Empty array as key ──────────────────────────────────────────────
(let [m @{}]
  (put m [] "empty")
  (assert (= (get m []) "empty") "empty array key"))

# ── Mutable arrays rejected as keys ─────────────────────────────────
(let [m @{}
      ok (first (protect (put m @[1 2] "x")))]
  (assert (not ok) "mutable array rejected as key"))

# ── frequencies with array values ────────────────────────────────────
(let [data [[1 2] [3 4] [1 2] [3 4] [1 2]]
      freq (frequencies data)]
  (assert (= (get freq [1 2]) 3) "frequencies with array values")
  (assert (= (get freq [3 4]) 2) "frequencies with array values 2"))

# ── keys round-trips ────────────────────────────────────────────────
(let [m @{[1 2] "x"}
      ks (keys m)]
  (assert (= (first ks) [1 2]) "keys returns array key"))

(println "ok")
