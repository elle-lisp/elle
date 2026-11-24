# An unused parameter, in Datalog

<!-- audited: 2026-11-23 -->

How a value handed to a block parameter that no later step uses is released after the jump, as Datalog that runs on proof.

[Release](release.md) holds the Unused parameter clause, and
[the ownership rules](ownership.md) state the rule in words.
[Recur](recur.md) names the jumps that hand a value to a block parameter.

A jump ends the hold of the value it hands over, under the Recur rule. The
Unheld value rule releases a value only on an edge out of a step where the
value is live or made, and a jump does neither for its parameter. So without
this rule, a parameter that no later step uses has no release, and an `if`
whose value nothing reads keeps the value of each arm.

The clause releases only a value the fiber holds. A value read out of a
container keeps the container as its owner when a jump hands it on, so the
release belongs to that owner. A sink, an owner not passed and a crossing each
hand a value to `[:sink i p]`, `[:owner x p]` or `[:cross p]`. Another
instance or fiber takes that hold and releases the value, so the clause skips
those names.

In `up`, each arm makes a value and hands it to `r`, and nothing uses `r`. So
`r` is released on the edge out of each jump. In `ub`, each arm reads a value
out of `m` and hands it to `r`. The value stays `m`'s, so nothing is released:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))
  (def {:ownership ownership} ((import "./datalog.md")))
  (def {:releases releases} ((import "./release.md")))

  (def up (fence 'up :doc "r is released on the edge out of each jump."
    :source "(defn up [c]
  (if c                       # 0  the test
    (array 1)                 # 1  v is made   2  v is handed to r
    (array 2))                # 3  u is made   4  u is handed to r
  nil)                        # 5  the exit"
    :facts {:next  [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5]]
            :alloc [[:v 1] [:u 3]]
            :pass  [[:v :r 2] [:u :r 4]]}
    :check (fn [up] (= [[:r 2 5] [:r 4 5]] (releases up (ownership up))))))

  (def ub (fence 'ub :doc "A value read out of a container and handed to r is not released."
    :source "(defn ub [m c]
  (if c                       # 0  the test
    (get m 0)                 # 1  x is read out of m   2  x is handed to r
    (get m 1))                # 3  z is read out of m   4  z is handed to r
  nil)                        # 5  the exit"
    :facts {:next  [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5]]
            :param [[:m 0]]
            :read  [[:x :m 1] [:z :m 3]]
            :pass  [[:x :r 2] [:z :r 4]]
            :use   [[:m 1] [:m 3]]}
    :check (fn [ub] (empty? (releases ub (ownership ub))))))

  {:fences [up ub]})
```
