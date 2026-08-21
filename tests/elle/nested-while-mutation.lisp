(elle/epoch 12)
# ─────────────────────────────────────────────────────────────────────
# A nested `while` may advance the SAME mutable `var` as its enclosing
# `while` (a let-scoped cursor shared across both loops). The compiler
# must keep such a slot-mutated binding on its one slot everywhere: a
# nested loop must not fork a private version of it, or paths through
# the sibling if-arm (which never enter the inner loop) would read the
# fork's uninitialized slot at the branch merge.
#
# `group-direct` and `group-cursor` are the same algorithm — group a
# list into runs of consecutive non-:eq items. The direct variant's
# inner while advances the OUTER loop's `k`; the cursor variant walks a
# private cursor `j` and resyncs `k = j` afterward. Both must agree.
#
#   input:  [:eq :ch :ch :eq :ch]
#   want:   @[@[1 2] @[4]]
#
# The bare-mechanism pins live in src/hir/functionalize/tests.rs
# (nested_while_branch_arm_keeps_outer_loop_param and siblings).
# ─────────────────────────────────────────────────────────────────────

(defn group-direct [items]
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

(defn group-cursor [items]
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
(def want "@[@[1 2] @[4]]")

(assert (= (string (group-direct input)) want) "direct-advance grouping")
(assert (= (string (group-cursor input)) want) "private-cursor grouping")
(assert (= (string (group-direct input)) (string (group-cursor input)))
        "both variants agree")

# The mechanism, minimal: the assign-arm runs on the first iteration,
# so the inner loop has not yet executed when the outer loop-head
# re-reads k. The read must see the arm's assignment.
(defn count-up []
  (def @k 0)
  (while (< k 5)
    (if (= k 0)
      (assign k (+ k 1))
      (while (< k 5) (assign k (+ k 1)))))
  k)
(assert (= (count-up) 5) "outer loop-head sees the arm assignment")

(println "ok")
