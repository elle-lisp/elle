(elle/epoch 12)
# A name a DESTRUCTURING pattern introduces records no release route.
#
# Four sites record one — `Define`, `Let`, `Letrec`, and the lambda prologue — and
# a pattern name is none of them: the pattern extracts the name from its
# scrutinee, and no binder stores it into a slot a value-routed release ever
# loads. So reassigning such a name says nothing about where the SCRUTINEE's own
# release may go: the scrutinee routes through the temp that produced it, which
# nothing repoints (docs/impl/region/mechanism.md § "A mutated holder poisons its
# value route, not its cell box").
#
# Reading the mutation off the pattern name instead refused the scrutinee's region
# from the frame-held admission, so the branch-arm window could not anchor its
# release and every path through the branch held the whole input.

# ── 1. The reassigned pattern name — both arms read the pair ────────────
# `xs` is live into the branch and both arms name it through the pattern's two
# names, so its single release has to land where every arm reaches it.
(defn split-head [t xs]
  (def (@a b) xs)
  (assign a 99)
  (if t (list a b) (list b a)))

(assert (= (split-head true (list 1 2)) (list 99 2))
        "the reassigned pattern name carries its new value")
(assert (= (split-head false (list 1 2)) (list 2 99))
        "the sibling arm reads the same destructured pair")

# ── 2. Not over-freed — the scrutinee outlives the reassignment ─────────
# The release the admission unlocks fires on paths that used to run none, so the
# read-back is the over-free face: a scrutinee freed at the merge would leave
# these reads walking a reclaimed page.
(defn read-back [t]
  (let [xs (list 4 5 6)]
    (def (@h a b) xs)
    (assign h 0)
    (if t (list h (first xs) (length xs) (+ a b)) (list (length xs) h))))

(assert (= (read-back true) (list 0 4 3 11))
        "the destructured scrutinee stays readable after its name is repointed")
(assert (= (read-back false) (list 3 0))
        "the sibling arm reads the scrutinee back too")

# ── 3. Bounded — the input does not ride the frame ──────────────────────
# A refusal here strands one whole input list per call, on whichever path the
# `decref_point` does not sit in. Both arms are driven so neither path hides it.
(defn drive [reps]
  (var k 0)
  (while (< k reps)
    (split-head true (list k 2))
    (split-head false (list k 2))
    (read-back true)
    (read-back false)
    (assign k (+ k 1))))

(defn growth [f]
  (f 20)
  (var before (arena/region-count))
  (f 200)
  (%sub (arena/region-count) before))

(let [rate (growth (fn [reps] (drive reps)))]
  (assert (%lt rate 100)
          (string "the destructured scrutinee is stranded per call: live count "
                  "grew by " rate " over 200 calls (expected flat)")))

(println "region-destructured-cursor: ok")
