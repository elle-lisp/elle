# A closure handed over, in Datalog

<!-- audited: 2026-11-22 -->

How a closure a jump hands on stays live for the checks of its captures, and is refused where it outlives a borrow, as Datalog on proof.

[Capture](capture.md) holds the Mutable capture clauses, which read
`live-as(k, q)`, and the Borrow before release clause reads it here.
[Liveness](liveness.md) derives `live-as` under the Waits follow a hand-over
rule, and [the ownership rules](ownership.md) state the Closure handed over and
Borrow before release rules in words.

| Derived | Holds when |
|---------|------------|
| `refused(x, k, q)` | the closure `k` is live at step `q`, where the region of `x`, which it captures, has ended |

| Rule | Datalog |
|------|---------|
| Borrow before release | `refused(x, k, q) :- capture(x, k, a), gone(x, q), live-as(k, q).` |

The rule reads `gone` and `live-as` from the final pass of
[captures](capture.md). A late capture moves a value into the closure, so the
closure stays live while that value is used. In `bb`, the first pass judges the
capture of `x` late, because `x` is released on entry to the `else` arm, where
`k` is used. In the final pass, `k` holds `x` and stays live on the `then` arm
until the use of `x` at step 5. But `y`, which `k` borrows, is released after
its use at step 4, so the rule refuses the program. No closure of `c1`, `c2`,
`c4`, `lc` or `l5` in [captures](capture.md) outlives a value it captures.

A jump hands a closure to a block parameter at the closure's last use, so
`live(k, q)` ends at the jump. A call through the parameter then runs the
closure where `live` no longer reaches it. So `late` and the Borrow before
release refusal read `live-as(k, q)`, which holds while a parameter that
carries `k` is live.

In `ck`, the `then` arm hands `k` to `r`, which runs after the last use of
`x`. So the capture is late, and `x` moves into `k`. `x` is then used through
`k` after the join, so `k` is live after the jump, and
[Hand-over at last use](handover.md) reads `r` out of `k` on that path. The
`else` arm hands `j` to `r`, so `r` is in two states after the join, and the
Join rule's target refuses `ck`.

In `bh`, the first pass sees `x` handed to `s`, so `x` has no release, and the
capture is not late. `x` is used after the join, so Hand-over at last use reads
`s` out of `x`, and `x` is released after its use at step 8. `r` still carries
`k`, which borrows `x`, when it runs at step 9, so the Borrow before release
rule refuses `bh`:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence :fence-from fence-from} ((import "../spec.md")))
  (def {:ownership ownership} ((import "./datalog.md")))
  (def {:live-as-of live-as-of} ((import "./liveness.md")))
  (def {:released-from released-from :two-states two-states} ((import "./release.md")))
  (def capture ((import "./capture.md")))
  (def {:with-captures with-captures :live-all live-all :gone-of gone-of} capture)

  (defn borrow-refusals [facts & waits]
    "refused(x, k, q) as [x k q], under the Borrow before release rule, read from the final
     pass with WAITS."
    (def all (with-captures facts ;waits))
    (def owned (ownership all))
    (def live-r (live-all all ;waits))
    (def gone (proof:relation (map (fn [xq] [xq true])
                                   (gone-of all owned (released-from all owned live-r)))))
    # capture(x, k, a), live-as(k, q), gone(x, q)
    (def by-closure (proof:relation (map (fn [[x k a]] [k x]) (get facts :capture []))))
    (def k-live (proof:join by-closure (live-as-of all live-r) (fn [k x q] [[x q] k])))
    (proof:elements (proof:join k-live gone (fn [[x q] k _] [x k q]))))

  (defn borrows-none [name]
    "The fence NAME of capture, built here: its closure outlives no value it captures."
    (fence-from capture name :doc (string "No closure in " name " outlives a value it captures.")
                :check (fn [facts] (empty? (borrow-refusals facts)))))

  (def bb (fence 'bb :doc "The capture of x is late, and k holds a borrow of y after its release."
    :source "(defn bb [c]
  (def x @[1])                # 0  x is made
  (def y @[2])                # 1  y is made
  (def k (fn [] (+ (length x) (length y))))   # 2  k is made, and captures x and y
  (if c                       # 3  the test
    (begin (length y)         # 4  y is used
           (length x))        # 5  x is used
    (begin (k)                # 6  k is used
           (length y))))      # 7  y is used
                              # 8  the exit"
    :facts {:next    [[0 1] [1 2] [2 3] [3 4] [4 5] [5 8] [3 6] [6 7] [7 8]]
            :alloc   [[:x 0] [:y 1] [:k 2]]
            :capture [[:x :k 2] [:y :k 2]]
            :use     [[:y 4] [:x 5] [:k 6] [:y 7]]}
    :check (fn [bb] (and (= [[:x :k 2]] ((with-captures bb) :store))
                         (= [[:y :k 5]] (borrow-refusals bb))))))

  (def ck (fence 'ck
    :doc "r carries k past the release of x, so the capture is late, and r has two states after the join."
    :source "(defn ck [c]
  (def x @[1])                 # 0  x is made
  (def k (fn [] (length x)))   # 1  k is made, and captures x
  (def r (if c k (fn [] 0)))   # 2  the test   3  k jumps to r   4  j is made   5  j jumps to r
  (length x)                   # 6  x is used
  (r))                         # 7  r is used
                               # 8  the exit"
    :facts {:next    [[0 1] [1 2] [2 3] [3 6] [2 4] [4 5] [5 6] [6 7] [7 8]]
            :alloc   [[:x 0] [:k 1] [:j 4]]
            :capture [[:x :k 1]]
            :pass    [[:k :r 3] [:j :r 5]]
            :use     [[:x 6] [:r 7]]}
    :check (fn [ck]
             (def ck2 (with-captures ck))
             (and (= [[:x :k 1]] (ck2 :store))
                  (= [[:r 6] [:r 7]] (two-states ck2 (ownership ck2) (live-all ck2)))))))

  (def bh (fence 'bh
    :doc "The capture of x is not late, and r carries k where the region of x has ended."
    :source "(defn bh [c]
  (def x @[1])                 # 0  x is made
  (def k (fn [] (length x)))   # 1  k is made, and captures x
  (def r (if c k k))           # 2  the test   3  k jumps to r   4  k jumps to r
  (def s (if c x x))           # 5  the test   6  x jumps to s   7  x jumps to s
  (length x)                   # 8  x is used
  (r))                         # 9  r is used
                               # 10 the exit"
    :facts {:next    [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5] [5 6] [5 7] [6 8] [7 8] [8 9] [9 10]]
            :alloc   [[:x 0] [:k 1]]
            :capture [[:x :k 1]]
            :pass    [[:k :r 3] [:k :r 4] [:x :s 6] [:x :s 7]]
            :use     [[:x 8] [:r 9]]}
    :check (fn [bh] (and (empty? ((with-captures bh) :store)) (= [[:x :k 9]] (borrow-refusals bh))))))

  {:borrow-refusals borrow-refusals
   :fences [bb ;(map borrows-none ['c1 'c2 'c4 'lc 'l5]) ck bh]})
```
