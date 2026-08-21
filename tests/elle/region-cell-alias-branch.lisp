(elle/epoch 12)
# A whole-value read of a fn-local 1-slot container taken through a BRANCH. What
# obliges the reader is the value it ends up holding, not the syntax that
# selected it: an arm that reads a container that re-stores borrows a reference
# the next `assign` releases, so the binder takes a counted one of its own
# (docs/impl/region/bindings.md § "A branch is a read of whichever arms read").
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

# ── 5. A MIXED branch — one arm reads, one allocates ────────────────────
# The replacement the counted read pays with is per-arm: the reading arm's
# regions are withdrawn from `keep`, so the container is its init's sole holder
# and donates, while the ALLOCATING arm's region stays — it is the only thing
# extending that value's last use out to the binder's retain. Both arms are read
# back after the loop has churned the container, which is where a cut region
# would show as a freed value under the reader.
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

# ── 6. A mixed branch whose other arm is a CALL result ──────────────────
# The arm that is not a read need not allocate in place: a call result carries
# its own placeholder release through the ANF temp's slot, and the reader's
# retain is balanced by the placeholder the counted read mints. Read back after
# the churn on both paths.
(defn make-pair [a]
  (array a (+ a 1)))

(defn churn-mixed-call [n]
  (let [@last (array 41 42)]
    (let [keep (if (< n 0) last (make-pair 5))]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (get keep 1) (get last 1)))))

(assert (= (churn-mixed-call 4) (list 6 7))
        "the call-result arm reads back beside the churned container")
(assert (= (churn-mixed-call -1) (list 42 42))
        "the reading arm of the call-result branch reads back its init")

# ── 7. A mixed branch whose other arm carries no reference ──────────────
# An immediate path holds nothing, so the binder's retain and the placeholder's
# release are both no-ops on it, and the reading arm still hands the donation
# back. `nil` is the same case with no value of its own to read.
(defn churn-mixed-immediate [n]
  (let [@last (array 41 42)]
    (let [keep (if (< n 0) last 99)]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (if (array? keep) (get keep 1) keep) (get last 1)))))

(assert (= (churn-mixed-immediate 4) (list 99 7))
        "the immediate arm carries no reference for the retain to name")
(assert (= (churn-mixed-immediate -1) (list 42 42))
        "the reading arm reads back its init beside the immediate arm")

# ── 8. A mixed match whose other arm is a rest-pattern binding ──────────
# The arm that is not a read produces a value the MATCH node itself allocated —
# the rest slice a `&` pattern binds. The counted read mints its placeholder at
# that same node, so this is the face that pins the two apart: the slice arm
# keeps its regions and reads back after the churn, and the reading arm still
# hands the container its donation.
(defn churn-mixed-rest [n xs]
  (let [@last (array 41 42)]
    (let [keep (match xs
                 [a & rest] rest
                 _ last)]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (length keep) (get last 1)))))

(assert (= (churn-mixed-rest 4 @[1 2 3]) (list 2 7))
        "the rest-pattern arm reads back its slice after the churn")
(assert (= (churn-mixed-rest 4 nil) (list 2 7))
        "the reading arm of the rest-pattern match reads back the container")

# ── 9. A statement wrapper around the read ─────────────────────────────
# A `begin` selects a value exactly as an arm does, and its own value is its
# tail's, so the reader ends up holding what the tail read and takes the same
# counted reference.
(defn churn-begin [n]
  (let [@last (array 41 42)]
    (let [keep (begin
                 1
                 last)]
      (var i 0)
      (while (< i n)
        (assign last (array i 7))
        (assign i (+ i 1)))
      (list (get keep 1) (get last 1)))))

(assert (= (churn-begin 4) (list 42 7))
        "the begin-wrapped alias survives every overwrite the loop makes")

# ── 10. Bounded ────────────────────────────────────────────────────────
# Each face strands its whole object graph per call when the producer's reference
# has no release route: the cursor's cons chain, the churn's displaced init. The
# mixed faces are the same bar — the arm that reads hands the donation back
# whether or not its sibling allocates.
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
      mixed-rate (growth (fn [k] (churn-mixed 4)))
      mixed-call-rate (growth (fn [k] (churn-mixed-call 4)))
      mixed-imm-rate (growth (fn [k] (churn-mixed-immediate 4)))
      mixed-rest-rate (growth (fn [k] (churn-mixed-rest 4 @[1 2 3])))
      begin-rate (growth (fn [k] (churn-begin 4)))]
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
  (assert (%lt mixed-rate 100)
          (string "the mixed branch strands its displaced init: live count grew by "
                  mixed-rate " over 200 calls (expected flat)"))
  (assert (%lt mixed-call-rate 100)
          (string "the call-result mixed branch strands its displaced init: live "
                  "count grew by " mixed-call-rate
                  " over 200 calls (expected flat)"))
  (assert (%lt mixed-imm-rate 100)
          (string "the immediate mixed branch strands its displaced init: live "
                  "count grew by " mixed-imm-rate
                  " over 200 calls (expected flat)"))
  (assert (%lt mixed-rest-rate 100)
          (string "the rest-pattern mixed match strands its displaced init: live "
                  "count grew by " mixed-rest-rate
                  " over 200 calls (expected flat)"))
  (assert (%lt begin-rate 100)
          (string "the begin-wrapped read strands its displaced init: live "
                  "count grew by " begin-rate " over 200 calls (expected flat)")))

(println "region-cell-alias-branch: ok")
