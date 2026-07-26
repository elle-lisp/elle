(elle/epoch 12)
# The return frontier is PER-PATH (docs/impl/region/mechanism.md § "The return
# frontier is per-path").
#
# A region a function returns is the caller's to free — the return mint hands over
# one owning reference and the caller's release consumes it. That is true only of
# the paths that actually reach the return with the value. Take a branch where one
# arm returns a heap value and a sibling arm does not: on the sibling path no mint
# fired, the caller receives nothing, and the callee still holds the only reference
# in existence. Its release must fire there.
#
# The value's single `decref_point` is its textually-last use, which lands in the
# returning arm — so without a per-path compensating release on the sibling arm
# nothing frees the region at all, and the free cascade that never runs strands
# every member with it (a 3-element list costs 3 objects per call, a struct 1).
#
# Both faces are pinned here, because the fix must not become an over-free:
#   LEAK face   — call each subject so the arm that does NOT carry the value out
#                 is the one taken; the object count must stay bounded.
#   UAF face    — call it so the value IS returned, then USE the result; the
#                 caller's reference must still be live (run under
#                 `--trace=guardfree` by `region_return_arm_escape_uaf`).
#
# The dual shape is the base case of a walk: `(if (= i 0) xs (go (- i 1) xs))`
# uses `xs` in BOTH arms, so the recursive arm's later use wins the `decref_point`
# and the base case is left with a mint and no release. That is the shape every
# `letrec` walk over a heap argument takes, so it is pinned here beside the
# immediate-sibling shape — with and without the arg threaded into the recursive
# call, and over a mutually-recursive pair.
#
# Controls bracket the diagnosis: a sibling arm that merely READS the value
# already compensates (`used-arm`), and a function with no branch at all
# (`plain-id`) never strands its argument — so the trigger is specifically the
# branch arm that carries the value across the return frontier.

(defn measure (thunk warm window)
  (var i 0)
  (while (%lt i warm)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/count))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (%sub (arena/count) before))

# ── subjects ──────────────────────────────────────────────────────
# A heap PARAM returned from the then-arm; the else-arm carries an immediate out.
(defn pick-param (i xs)
  (if (%eq i 0) xs 7))
# The mirror: the value leaves through the else-arm.
(defn pick-param-else (i xs)
  (if (%eq i 0) 7 xs))
# A sibling arm that reads but does not return the value — already compensated.
(defn used-arm (i xs)
  (if (%eq i 0) (length xs) 7))
# No branch at all — the plain pass-through control.
(defn plain-id (xs)
  xs)
# The walk base case: both arms use `xs`, so the recursive arm holds the
# `decref_point` and the returning arm carries a mint with no release.
(defn walk-hold (i xs)
  (if (%eq i 0) xs (walk-hold (%sub i 1) xs)))
# The same with the arg consumed by the recursive call rather than threaded on —
# the `(rest xs)` shape stdlib `drop` takes.
(defn walk-rest (i xs)
  (if (%eq i 0) xs (walk-rest (%sub i 1) (rest xs))))
# Mutual recursion: the sibling arm's use is a call to the peer, not to self.
(defn walk-b (i xs)
  (if (%eq i 0) xs (walk-a (%sub i 1) xs)))
(defn walk-a (i xs)
  (if (%eq i 0) xs (walk-b (%sub i 1) xs)))
# A LOCAL heap value (not a param) returned from one arm: the region is born in
# this activation, so on the non-returning path there is no caller to inherit it
# and no callee-side release either.
(defn local-hold (i)
  (let [xs (list i 2 3)]
    (if (%eq i 0) xs 7)))

# ── controls: bounded already ─────────────────────────────────────
(def c-used (measure (fn () (used-arm 1 (list 1 2 3))) 200 2000))
(def c-plain (measure (fn () (plain-id (list 1 2 3))) 200 2000))
(def c-carried (measure (fn () (pick-param 0 (list 1 2 3))) 200 2000))
(assert (%lt c-used 200)
        (concat "control: read-only sibling arm strands the arg, delta="
                (number->string c-used)))
(assert (%lt c-plain 200)
        (concat "control: branchless pass-through strands the arg, delta="
                (number->string c-plain)))
(assert (%lt c-carried 200)
        (concat "control: the arm that DOES return the value strands it, delta="
                (number->string c-carried)))

# ── leak face: the arm that does not carry the value out ──────────
(def w-then (measure (fn () (pick-param 1 (list 1 2 3))) 200 2000))
(def w-else (measure (fn () (pick-param-else 0 (list 1 2 3))) 200 2000))
(def w-struct (measure (fn () (pick-param 1 {:a 1 :b 2})) 200 2000))
(println "region-return-arm-escape-leak deltas over 2000 iters:")
(println "  else-arm taken, list arg:   " w-then)
(println "  then-arm taken, list arg:   " w-else)
(println "  else-arm taken, struct arg: " w-struct)
(assert (%lt w-then 200)
        (concat "the non-returning arm strands the returned param's region, delta="
                (number->string w-then)))
(assert (%lt w-else 200)
        (concat "the non-returning arm strands the returned param's region "
                "(mirrored arms), delta=" (number->string w-else)))
(assert (%lt w-struct 200)
        (concat "the non-returning arm strands a struct arg's region, delta="
                (number->string w-struct)))

# ── leak face: the returning arm whose decref_point sits in its sibling ──
(def w-hold (measure (fn () (walk-hold 1 (list 1 2 3))) 200 2000))
(def w-rest (measure (fn () (walk-rest 1 (list 1 2 3))) 200 2000))
(def w-mutual (measure (fn () (walk-a 1 (list 1 2 3))) 200 2000))
(def w-local (measure (fn () (local-hold 1)) 200 2000))
(println "  walk base case, arg threaded: " w-hold)
(println "  walk base case, arg consumed: " w-rest)
(println "  walk base case, mutual:       " w-mutual)
(println "  non-returning arm, local value: " w-local)
(assert (%lt w-hold 200)
        (concat "the walk's base case strands its arg (decref_point in the "
                "recursive arm), delta=" (number->string w-hold)))
(assert (%lt w-rest 200)
        (concat "the walk's base case strands its arg with the arg consumed by "
                "the recursive call, delta=" (number->string w-rest)))
(assert (%lt w-mutual 200)
        (concat "the mutually-recursive walk's base case strands its arg, delta="
                (number->string w-mutual)))
(assert (%lt w-local 200)
        (concat "a locally-allocated returned value strands on the arm that does "
                "not return it, delta=" (number->string w-local)))

# ── UAF face: the returned value must survive the hand-over ───────
# Drives the returning path many times and READS the result each time. A
# compensating release that fired on the wrong path frees the value under this
# read; under `--trace=guardfree` the stale deref detonates, and the length check
# catches a silent recycle on the plain tiers.
(var seen 0)
(var k 0)
(while (%lt k 2000)
  (let [r (pick-param 0 (list 1 2 3))]
    (assign seen (%add seen (length r))))
  (assign k (%add k 1)))
(assert (%eq seen 6000)
        (concat "returned value did not survive the caller's use, sum="
                (number->string seen)))

(var seen2 0)
(var m 0)
(while (%lt m 2000)
  (let [r (pick-param-else 1 (list 4 5))]
    (assign seen2 (%add seen2 (length r))))
  (assign m (%add m 1)))
(assert (%eq seen2 4000)
        (concat "returned value did not survive the caller's use (mirrored arms), "
                "sum=" (number->string seen2)))

# The walk's base case, driven through BOTH arms alternately so a per-arm release
# on the wrong path frees a value the other path still hands back. `walk-rest`
# shortens the list on the way down, so the length also witnesses the arg the
# recursive arm consumed.
(var seen3 0)
(var n 0)
(while (%lt n 2000)
  (let [r (walk-hold 0 (list 1 2 3))]
    (assign seen3 (%add seen3 (length r))))
  (let [r2 (walk-rest 1 (list 1 2 3))]
    (assign seen3 (%add seen3 (length r2))))
  (let [r3 (walk-a 2 (list 7))]
    (assign seen3 (%add seen3 (length r3))))
  (assign n (%add n 1)))
(assert (%eq seen3 12000)
        (concat "a walk's returned value did not survive the caller's use, sum="
                (number->string seen3)))

(println "region-return-arm-escape-leak: ok")
