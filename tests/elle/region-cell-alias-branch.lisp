(elle/epoch 12)
# A whole-value read of a fn-local 1-slot container taken through a BRANCH. What
# obliges the reader is the value it ends up holding, not the syntax that
# selected it: every arm here reads a container that re-stores, so on every path
# the name borrows a reference the next `assign` releases, and the binder takes a
# counted one of its own (docs/impl/region/bindings.md § "A branch whose every arm
# is such a read is one too").
#
# What the counted read is worth is the DONATION. Reading the branch as an
# ordinary alias leaves the reader a holder of the container's init region, so the
# container falls back to counting its init — and that route releases the
# producer's reference through the slot recorded for the init region, which in
# this ordering is the container's own. A mutated slot is no release route, so the
# reference has none and the shape strands its whole object graph per call.
#
# tests/elle/region-cell-alias-after.lisp is the same family with the alias bound
# by a bare read. The two files together isolate the branch from the model.

# ── 1. The cursor walk, alias taken through a two-armed branch ──────────
# Both arms read the same container, so the branch is a whole-value read of it
# however the condition falls.
(defn cursor-branch [k]
  (var r (list 1 2 3 4 5))
  (let [keep (if (< k 0) r r)]
    (var seen 0)
    (while (not (empty? r))
      (assign seen (+ seen (first r)))
      (assign r (rest r)))
    (list seen (first keep) (length keep))))

(assert (= (cursor-branch 0) (list 15 1 5))
        "the branch alias reads back the chain head the cursor walked off")

# ── 2. Two DIFFERENT containers, one per arm ────────────────────────────
# The retain names the runtime value, so one instruction at the binder covers
# whichever arm ran; each container keeps its own donation, and the one that did
# not run was never aliased at runtime.
(defn cursor-two [k]
  (var r (list 1 2 3))
  (var s (list 7 8 9 10))
  (let [keep (if (< k 0) r s)]
    (var n 0)
    (while (not (empty? r))
      (assign n (+ n 1))
      (assign r (rest r)))
    (while (not (empty? s))
      (assign n (+ n 1))
      (assign s (rest s)))
    (list n (first keep) (length keep))))

(assert (= (cursor-two 0) (list 7 7 4))
        "the arm that ran is the one read back, after both cursors walked off")
(assert (= (cursor-two -1) (list 7 1 3))
        "the other arm reads back its own container's head")

# ── 3. A cond with an else clause, over a churned container ─────────────
# Every clause body reads the container and the else clause supplies the missing
# path, so no path produces something else.
(defn churn-cond [n]
  (let [@last (array 41 42)]
    (let [keep (cond
                 (< n 0) last
                 (> n 100) last
                 last)]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (get keep 1) (get last 1)))))

(assert (= (churn-cond 0) (list 42 42))
        "an unrun loop reads the init back through both names")
(assert (= (churn-cond 4) (list 42 7))
        "the cond alias survives every overwrite the loop makes")

# ── 4. A match, whose arms are every value-producing path ───────────────
# An unmatched value signals rather than falling through, so a `match` needs no
# else clause for every path to be accounted for. The arms' pattern names are
# irrelevant — what is asked of an arm is what its body produces.
(defn churn-match [n]
  (let [@last (array 41 42)]
    (let [keep (match n
                 0 last
                 _ last)]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (get keep 1) (get last 1)))))

(assert (= (churn-match 0) (list 42 42))
        "the match alias reads back an init the loop never displaced")
(assert (= (churn-match 4) (list 42 7))
        "the match alias survives every overwrite the loop makes")

# ── 5. Control — a MIXED branch, declined whole ─────────────────────────
# One arm allocates, so the branch is not a whole-value read and the container
# counts its init instead. Read back to pin that the decline is a lifetime
# decision, never a correctness one.
(defn churn-mixed [n]
  (let [@last (array 41 42)]
    (let [keep (if (< n 0) last (array 5 5))]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (get keep 1) (get last 1)))))

(assert (= (churn-mixed 4) (list 5 7))
        "the mixed branch's allocating arm reads back beside the container")
(assert (= (churn-mixed -1) (list 42 42))
        "the mixed branch's reading arm reads back the undisplaced init")

# ── 6. Bounded ─────────────────────────────────────────────────────────
# Each counted face strands its whole object graph per call when the producer's
# reference has no release route: the cursor's cons chain, the churn's displaced
# init. The mixed control is on the counted-init route and carries that route's
# own over-keep, so it is pinned only against getting worse.
(defn drive [f reps]
  (var k 0)
  (while (< k reps)
    (f k)
    (assign k (+ k 1))))

(defn growth [f]
  (drive f 20)
  (var before (arena/region-count))
  (drive f 200)
  (%sub (arena/region-count) before))

(let [branch-rate (growth (fn [k] (cursor-branch k)))
      two-rate (growth (fn [k] (cursor-two k)))
      cond-rate (growth (fn [k] (churn-cond 4)))
      match-rate (growth (fn [k] (churn-match 4)))
      mixed-rate (growth (fn [k] (churn-mixed 4)))]
  (assert (%lt branch-rate 100)
          (string "the branch cursor strands its chain: live count grew by "
                  branch-rate " over 200 calls (expected flat)"))
  (assert (%lt two-rate 100)
          (string "the two-container branch strands a chain: live count grew by "
                  two-rate " over 200 calls (expected flat)"))
  (assert (%lt cond-rate 100)
          (string "the cond alias strands its displaced init: live count grew by "
                  cond-rate " over 200 calls (expected flat)"))
  (assert (%lt match-rate 100)
          (string "the match alias strands its displaced init: live count grew by "
                  match-rate " over 200 calls (expected flat)"))
  (assert (%lt mixed-rate 300)
          (string "the mixed-branch control regressed: live count grew by "
                  mixed-rate " over 200 calls")))

(println "region-cell-alias-branch: ok")
