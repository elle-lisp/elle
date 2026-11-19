# Recur in Datalog

<!-- audited: 2026-11-19 -->

How a loop hands each new value to its parameter, so the value of the last pass is released at its last use, as Datalog that runs on proof.

[Datalog](datalog.md) holds the `pass` fact and the Recur clauses for
`owned`, and [release](release.md) holds the one for `live`.
[The ownership rules](ownership.md) state the Recur rule in words.

A loop parameter names one value on each pass. The step that recurs hands the
new value over, so the new value has no release of its own, and the old value's
hold ends there. The old value is released at its last use in its pass, or
later where a new value lies inside it. Each loop below has one state at its
head.

`l1` walks a list it made. Each new `xs` is read out of the old one, so it
keeps the old one's owner, and the old one waits for it. `xs` is released once,
on the edge out of the loop:

```lisp
(include-file "release.md")

(defn one-state? [facts]
  "Whether no value is live in two states anywhere in FACTS."
  (def owned (ownership facts))
  (empty? (two-states facts owned (liveness facts owned))))

(def loop-next [[0 1] [1 2] [2 3] [3 4] [4 1] [2 5] [5 6]])

# (defn l1 []
#   (def xs (list 1 2 3))       # 0  xs is made
#   (loop [xs xs]               # 1  the loop head
#     (if (pair? xs)            # 2  xs is used
#       (recur (rest xs))       # 3  ys is read out of xs   4  ys is handed to xs
#       nil)))                  # 5  the exit path          6  the exit
(def l1 {:next loop-next
         :alloc [[:xs 0]]
         :read  [[:ys :xs 3]]
         :pass  [[:ys :xs 4]]
         :use   [[:xs 2] [:xs 3]]})
(assert (one-state? l1) "the new xs keeps the old one's owner")
(assert (= [[:xs 2 5]] (releases l1 (ownership l1))) "xs is released once, after the loop")
```

`l2` makes a new value on each pass, so each pass releases the old `a` after its
use at step 2. `l3` builds an accumulator: the Allocation operand rule moves the
old `acc` into the new pair, so nothing is released inside the loop:

```lisp
# (defn l2 [c]
#   (loop [a (array 1)]         # 0  a is made   1  the loop head
#     (length a)                # 2  a is used
#     (if c                     #    the test is part of step 2
#       (recur (array 2))       # 3  b is made   4  b is handed to a
#       nil)))                  # 5  the exit path   6  the exit
(def l2 {:next loop-next
         :alloc [[:a 0] [:b 3]]
         :pass  [[:b :a 4]]
         :use   [[:a 2]]})
(assert (one-state? l2) "a has one owner at the loop head")
(assert (= [[:a 2 3] [:a 2 5]] (releases l2 (ownership l2)))
        "each pass releases its a after the last use")

# (defn l3 [c]
#   (loop [acc (array 0)]       # 0  acc is made   1  the loop head
#     (if c                     # 2  the test
#       (recur (pair 1 acc))    # 3  v is made, with acc as an operand   4  v is handed to acc
#       (length acc))))         # 5  acc is used   6  the exit
(def l3 {:next      loop-next
         :alloc     [[:acc 0] [:v 3]]
         :operand   [[:acc :v 3]]
         :immutable [:v]
         :pass      [[:v :acc 4]]
         :use       [[:acc 3] [:acc 5]]})
(assert (one-state? l3) "acc has one owner at the loop head")
(assert (= [[:acc 5 6]] (releases l3 (ownership l3))) "acc is released after its use on exit")
```

`l4` stores `xs` into `acc` before it recurs on `(rest xs)`, so from the second
pass on, `acc` holds each `xs`. The last Recur clause brings that owner back to
the head, where the fiber holds the first `xs`. So the loop has two states at
its head, and the Join rule's target refuses it. Without the clause, the exit
path would release an `xs` that `acc` holds:

```lisp
# (defn l4 []
#   (def acc @[])               # 0  acc is made
#   (loop [xs (list 1 2)]       # 1  xs is made   2  the loop head
#     (if (pair? xs)            # 3  xs is used
#       (begin (push acc xs)    # 4  xs is stored into acc
#              (recur (rest xs)))   # 5  ys is read out of xs   6  ys is handed to xs
#       (length xs))))          # 7  xs is used   8  the exit
(def l4 {:next  [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 2] [3 7] [7 8]]
         :alloc [[:acc 0] [:xs 1]]
         :store [[:xs :acc 4]]
         :read  [[:ys :xs 5]]
         :pass  [[:ys :xs 6]]
         :use   [[:xs 3] [:acc 4] [:xs 4] [:xs 5] [:xs 7]]})
(assert (not (one-state? l4)) "xs is in two states at the loop head")
```
