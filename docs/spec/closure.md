# A closure handed over, in Datalog

<!-- audited: 2026-11-20 -->

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
through `k` after the join, so `k` is live there, and its release after that
use sits on the edge both paths share. At step 7, `r` still carries `k`, whose
region has ended, so the Borrow before release rule refuses `ck`:

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
(assert (= [[:x :k 7]] (borrow-refusals ck)) "and r carries k where the region of x has ended")
```
