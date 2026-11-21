(elle/epoch 18)
# audited: 2026-11-20
# A native tail call that a pass moved out of tail position takes one return mint, at the HIR Return node.
# docs/impl/region/returnmint.md
#
# `mark_tail_calls` marks a call before functionalization, and
# `wrap_tail_returns` puts the HIR `Return` node around a returned value after
# ANF. Two shapes leave a marked native tail call outside the tail position that
# walk reads, with its value still leaving through a `Return` node: an arm of a
# tail branch whose arm assigns a mutable binding (the branch's value is bound,
# and the binding's versions merge after it), and the last operand of a tail
# `and`/`or` (the node wraps the whole form). The counter-factual: the
# post-`TailCall` fall-through retain fires beside the `Return` node's mint,
# and each call strands its result's region.
#
# The trap: a subject called by name is grafted into the measuring thunk,
# where the call is no tail call at all and reads 0 whatever the lowering
# does. Each subject is therefore reached through a reassigned binding, which
# no graft resolves.

(def window 2000)

(include-file "gauge/window.lisp")

(var slot nil)
(defn opaque [f]
  "F, behind a binding the graft cannot resolve."
  (assign slot f)
  slot)

# subjects ─────────────────────────────────────────────────────────────────────
# A tail `if` whose taken arm assigns `xs`: functionalization merges `xs`
# after the branch, so the arm's `[xs]` (a native `array` call) is no longer
# the function's tail.
(def if-assign
  (opaque (fn [c]
            (var xs 0)
            (if c
              7
              (begin
                (assign xs 1)
                [xs])))))
(def cond-assign
  (opaque (fn [c]
            (var xs 0)
            (cond
              c 7
              true
                (begin
                  (assign xs 1)
                  [xs])))))
(def match-assign
  (opaque (fn [c]
            (var xs 0)
            (match c
              true 7
              _
                (begin
                  (assign xs 1)
                  [xs])))))
# The assigning arm is the sibling: an `each` loop assigns its index, so the
# allocating arm's tail call moves as well.
(def if-sibling-each
  (opaque (fn [c]
            (def e @[1])
            (if c
              [e]
              (begin
                (each i in [1 2]
                  i)
                7)))))
# The last operand of a tail `and`/`or`.
(def or-last (opaque (fn [x] (or false (array x)))))
(def and-last (opaque (fn [x] (and true (array x)))))
# A pass-through native in the same position: its result is the element of a
# list built per call, so a surplus retain pins that element's region.
(def or-first (opaque (fn [xs] (or nil (first xs)))))

# The control: a native tail call in a tail arm that assigns nothing keeps its
# fall-through retain as the only mint.
(def if-plain (opaque (fn [c] (if c 7 (array c)))))

# The discriminator: a sink that keeps every result grows by one region per
# call, so a gauge that reads 0 there is dead.
(def sink @[])
(def keep
  (opaque (fn [x]
            (push sink (array x))
            nil)))

(def if-assign-d (measure-regions (fn () (if-assign false)) 200 window))
(def cond-assign-d (measure-regions (fn () (cond-assign false)) 200 window))
(def match-assign-d (measure-regions (fn () (match-assign false)) 200 window))
(def if-sibling-each-d
  (measure-regions (fn () (if-sibling-each true)) 200 window))
(def or-last-d (measure-regions (fn () (or-last 1)) 200 window))
(def and-last-d (measure-regions (fn () (and-last 1)) 200 window))
(def or-first-d
  (measure-regions (fn () (or-first (list (string "a")))) 200 window))
(def if-plain-d (measure-regions (fn () (if-plain false)) 200 window))
(def keep-d (measure-regions (fn () (keep 1)) 200 window))

(println "region-native-tail-moved-leak deltas over " window " iters:")
(println "  if " if-assign-d "  cond " cond-assign-d "  match " match-assign-d
         "  sibling each " if-sibling-each-d)
(println "  or " or-last-d "  and " and-last-d "  or first " or-first-d
         "  control " if-plain-d "  sink " keep-d)

# A surviving double mint reads one region per call, about 2000 here; 100 is
# slack for the measurement's one-time intercept.
(def bounded? (bounded-by 100))

(assert (> keep-d (- window 100))
        (string "the sink grows by one region per call, delta=" keep-d))
(bounded? if-plain-d "the control's plain tail arm")

(bounded? if-assign-d "a tail if arm beside an assign")
(bounded? cond-assign-d "a tail cond arm beside an assign")
(bounded? match-assign-d "a tail match arm beside an assign")
(bounded? if-sibling-each-d "a tail if arm beside an each loop")
(bounded? or-last-d "the last operand of a tail or")
(bounded? and-last-d "the last operand of a tail and")
(bounded? or-first-d "a pass-through native in a tail or")

(println "region-native-tail-moved-leak: ok")
