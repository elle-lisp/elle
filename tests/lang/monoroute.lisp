(elle/epoch 18)
# audited: 2026-11-23
# The stdlib push and put route each container to its monomorphic intrinsic, and each array push twin takes one mutability.
# docs/intrinsics.md
#
# Each route keeps the wrapper's meaning: an immutable input answers a fresh
# collection, and a mutable input mutates in place and answers itself. Each
# routed arm's op discharges its operand contract through the arm's
# narrowing on every compile of the stdlib; the narrowing itself is pinned in
# src/hir/typeinfer/tests/narrow.rs.

# ── push: array family ───────────────────────────────────────────────
# Immutable array → fresh array (the `:array → %push-array` arm).
(assert (= (push [1 2] 3) [1 2 3]) "push immutable array appends")
(let [a [1 2]]
  (push a 3)
  (assert (= a [1 2]) "push leaves the immutable source unchanged"))

# Mutable @array → in-place, returns the same value (`:@array → %push-array-mut`).
(let [a @[1 2]]
  (assert (= (push a 3) @[1 2 3]) "push @array appends in place")
  (assert (= a @[1 2 3]) "push @array mutated the source"))

# ── push: each array twin takes its own mutability ───────────────────
# Called as a value, a twin checks its array at run time and refuses the
# other mutability. A twin that took it would answer a fresh array where
# %push-array-mut declares the array itself, or append in place where
# %push-array declares a fresh array.
(def push-fresh %push-array)
(def push-in-place %push-array-mut)
(let [a @[1]]
  (assert (identical? (push-in-place a 2) a)
          "%push-array-mut answers its @array")
  (assert (= (push-fresh [1] 2) [1 2]) "%push-array answers a fresh array")
  (let [[ok? err] (protect (push-fresh a 3))]
    (assert (not ok?) "%push-array refuses an @array")
    (assert (= :type-error (get err :error)) "with a type error")
    (assert (= a @[1 2]) "and leaves the @array as it was"))
  (let [[ok? err] (protect (push-in-place [1] 2))]
    (assert (not ok?) "%push-array-mut refuses an immutable array")
    (assert (= :type-error (get err :error)) "with a type error")))

# ── push: the string and bytes families ──────────────────────────────
(assert (= (push "ab" "c") "abc") "push immutable string appends")
(let [s @"ab"]
  (push s "c")
  (assert (= s @"abc") "push @string appends in place"))
(assert (= (push (bytes 1 2) 3) (bytes 1 2 3)) "push immutable bytes appends")

# ── put: struct family ───────────────────────────────────────────────
# Immutable struct → fresh struct (the `:struct → %put-struct` arm).
(assert (= (put {:a 1} :b 2) {:a 1 :b 2}) "put immutable struct assoc")
(let [s {:a 1}]
  (put s :b 2)
  (assert (= s {:a 1}) "put leaves the immutable struct unchanged"))

# Mutable @struct → in-place (`:@struct → %put-struct-mut`).
(let [s @{:a 1}]
  (assert (= (put s :b 2) @{:a 1 :b 2}) "put @struct assoc in place")
  (assert (= s @{:a 1 :b 2}) "put @struct mutated the source"))

# ── put: array family (indexed set) ──────────────────────────────────
# Immutable array → fresh array (`:array → %put-array`).
(assert (= (put [1 2 3] 0 9) [9 2 3]) "put immutable array index")
(let [a [1 2 3]]
  (put a 0 9)
  (assert (= a [1 2 3]) "put leaves the immutable array unchanged"))

# Mutable @array → in-place (`:@array → %put-array-mut`).
(let [a @[1 2 3]]
  (assert (= (put a 0 9) @[9 2 3]) "put @array index in place")
  (assert (= a @[9 2 3]) "put @array mutated the source"))
