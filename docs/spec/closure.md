# A closure handed over, in Datalog

<!-- audited: 2026-11-21 -->

How a closure that a jump hands to a block parameter stays live for the checks of its captures, as Datalog that runs on proof.

[Capture](capture.md) holds the Mutable capture and Borrow before release
clauses, which read `live-as(k, q)`, and its fences are included here.
[Liveness](liveness.md) derives `live-as` under the Waits follow a hand-over
rule, and [the ownership rules](ownership.md) state the Closure handed over
rule in words.

A jump hands a closure to a block parameter at the closure's last use, so
`live(k, q)` ends at the jump. A call through the parameter then runs the
closure where `live` no longer reaches it. So `late` and the Borrow before
release refusal read `live-as(k, q)`, which holds while a parameter that
carries `k` is live.

In `ck` below, the `then` arm hands `k` to `r`, which runs after the last use
of `x`. So the capture is late, and `x` moves into `k`. `x` is then used
through `k` after the join, so `k` is live after the jump, and
[Hand-over at last use](handover.md) reads `r` out of `k` on that path. The
`else` arm hands `j` to `r`, so `r` is in two states after the join, and the
Join rule's target refuses `ck`:

```lisp
(include-file "capture.md")

# (defn ck [c]
#   (def x @[1])                 # 0  x is made
#   (def k (fn [] (length x)))   # 1  k is made, and captures x
#   (def r (if c k (fn [] 0)))   # 2  the test   3  k jumps to r   4  j is made   5  j jumps to r
#   (length x)                   # 6  x is used
#   (r))                         # 7  r is used
#                                # 8  the exit
(def ck {:next    [[0 1] [1 2] [2 3] [3 6] [2 4] [4 5] [5 6] [6 7] [7 8]]
         :alloc   [[:x 0] [:k 1] [:j 4]]
         :capture [[:x :k 1]]
         :pass    [[:k :r 3] [:j :r 5]]
         :use     [[:x 6] [:r 7]]})
(assert (= [[:x :k 1]] ((with-captures ck) :store))
        "r carries k past the release of x, so the capture is late")
(def ck2 (with-captures ck))
(assert (= [[:r 6] [:r 7]] (two-states ck2 (ownership ck2) (live-all ck2)))
        "r is read out of k on one path and handed j on the other")
```

In `bh` below, the first pass sees `x` handed to `s`, so `x` has no release,
and the capture is not late. `x` is used after the join, so Hand-over at last
use reads `s` out of `x`, and `x` is released after its use at step 8. `r`
still carries `k`, which borrows `x`, when it runs at step 9, so the Borrow
before release rule refuses `bh`:

```lisp
# (defn bh [c]
#   (def x @[1])                 # 0  x is made
#   (def k (fn [] (length x)))   # 1  k is made, and captures x
#   (def r (if c k k))           # 2  the test   3  k jumps to r   4  k jumps to r
#   (def s (if c x x))           # 5  the test   6  x jumps to s   7  x jumps to s
#   (length x)                   # 8  x is used
#   (r))                         # 9  r is used
#                                # 10 the exit
(def bh {:next    [[0 1] [1 2] [2 3] [2 4] [3 5] [4 5] [5 6] [5 7] [6 8] [7 8] [8 9] [9 10]]
         :alloc   [[:x 0] [:k 1]]
         :capture [[:x :k 1]]
         :pass    [[:k :r 3] [:k :r 4] [:x :s 6] [:x :s 7]]
         :use     [[:x 8] [:r 9]]})
(assert (empty? ((with-captures bh) :store)) "the capture of x is not late")
(assert (= [[:x :k 9]] (borrow-refusals bh)) "r carries k where the region of x has ended")
```
