(elle/epoch 12)
# ─────────────────────────────────────────────────────────────────────
# Elle bug: a nested `while` that assigns the SAME mutable `var` as its
# enclosing `while` (inside a function's `let` scope) does not work.
#
# `group-buggy` and `group-fixed` are the same algorithm. The only
# difference: the buggy one's inner `while` advances the OUTER loop's `k`
# directly; the fixed one walks with a PRIVATE cursor `j` and resyncs
# `k = j` afterward. The buggy version produces the wrong result.
#
# Discovered writing an LCS hunk-grouper (snide lib/diff.lisp). Single
# loops are unaffected; hoisting the var to a top-level `def` also makes
# the buggy shape work — it is specifically a let-scoped `var` shared
# across nested `while`s.
#
# Group a list into runs of consecutive non-:eq items.
#   input:  [:eq :ch :ch :eq :ch]
#   want:   @[@[1 2] @[4]]
#
# ─────────────────────────────────────────────────────────────────────

(defn group-buggy [items]
  (let [total (length items)
        result @[]]
    (def @k 0)
    (while (< k total)
      (if (= (get items k) :eq)
        (assign k (+ k 1))
        (let [run @[]]
          (while (and (< k total) (not (= (get items k) :eq)))
            (push run k)
            (assign k (+ k 1)))
          (push result run))))
    result))

(defn group-fixed [items]
  (let [total (length items)
        result @[]]
    (def @k 0)
    (while (< k total)
      (if (= (get items k) :eq)
        (assign k (+ k 1))
        (let [run @[]]
          (def @j k)
          (while (and (< j total) (not (= (get items j) :eq)))
            (push run j)
            (assign j (+ j 1)))
          (assign k j)
          (push result run))))
    result))

(def input [:eq :ch :ch :eq :ch])
(println "want:  @[@[1 2] @[4]]")
(println "buggy: " (group-buggy input))
(println "fixed: " (group-fixed input))
