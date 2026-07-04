(elle/epoch 12)
## Monomorphic data-op routing: stdlib `push`/`put` dispatch each container
## family to its monomorphic intrinsic via `(match (type-of coll) …)`.
## This file pins that the routing is
## **behaviour-preserving** on each tier: immutable inputs return a fresh
## collection, mutable inputs mutate in place and return the same value.
##
## It does NOT, by itself, exercise the monomorphization proof obligation
## (`check_monomorphic_proof_obligations`): the corpus runner is checked-on by
## default, and the obligation only fires on the silent (unchecked-intrinsics)
## path (`infer_and_rewrite` early-returns under checked). The obligation's
## discharge for these exact routed arms is pinned where the routing actually
## compiles silent: the unchecked stdlib build itself (the
## `runtime::tests::lifecycle::two_instances_*` Rust tests) plus the unit pins
## `proven_monomorphic_op_compiles_under_match_narrowing` /
## `match_typeof_arm_narrows_authoritatively_over_a_called_param`
## (`src/hir/typeinfer.rs`).

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

# ── push: byte-copy families keep the polymorphic op (no mono variant) ─
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
