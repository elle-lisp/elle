# Made anew, in Datalog

<!-- audited: 2026-11-23 -->

How a step that makes a value ends each hold on the value made before under its name, as Datalog that runs on proof.

[Datalog](datalog.md) holds the Made anew clause, and
[the ownership rules](ownership.md) state the rule in words.

A value is named by the binding that receives it, so each pass of a loop makes
a new value under the old name. An owner holds a name, and a hold the old value
took on an earlier pass would otherwise still hold the new one. The new value
would then have two owners, or keep an old owner live. A pass to a block
parameter already ends the parameter's holds, under the second Recur clause. A
read needs no clause: the value a read names has one owner, the container it is
read from, and that owner has the same name on every pass.

In `ms`, each pass makes `t` and stores it into a new `m`. Without the rule,
the `m` of the last pass still holds `t` where the next pass makes it, so `t` is
in two states, and `m` is released only on the way out of the loop.

In `ns`, each pass makes `n` with a new `m` as an operand. Without the rule,
the `n` of the last pass still holds `m`, so the new `m` keeps the old `n` live,
and the old `n` keeps the new `m` live where the pass makes it. Made again while
live then refuses both, and `n` too is released only on the way out.

A removal makes a value too. In `rs`, each pass takes `v` out of `stack` and
stores it into `log`, which holds the `v` of every earlier pass:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))
  (def {:ownership ownership} ((import "./datalog.md")))
  (def {:liveness liveness :operand operand} ((import "./liveness.md")))
  (def {:two-states two-states :released-from released-from :releases releases}
    ((import "./release.md")))
  (def {:made-again made-again} ((import "./again.md")))

  (defn in-two-states [facts]
    "refused(v, p) as [v p], under the Join rule."
    (two-states facts (ownership facts) (liveness facts (ownership facts))))

  (def ms (fence 'ms :doc "The new t has one owner, and each pass releases its m."
    :source "(defn ms [c]
  (while c               # 0  the loop head, and the test
    (let [m @[@[1]]]     # 1  t is made   2  m is made, with t as an operand
      (length m))))      # 3  m is used, and the loop goes back to 0
                         # 4  the exit"
    :facts {:next    [[0 1] [1 2] [2 3] [3 0] [0 4]]
            :alloc   [[:t 1] [:m 2]]
            :operand [[:t :m 2]]
            :use     [[:t 2] [:m 3]]}
    :check (fn [ms] (and (empty? (in-two-states ms)) (= [[:m 3 0]] (releases ms (ownership ms)))))))

  (def ns (fence 'ns :doc "No value is made while the old one is live, and each pass releases its n."
    :source "(defn ns [c]
  (while c               # 0  the loop head, and the test
    (let [n [(string c)]] # 1  m is made   2  n is made, with m as an operand
      (length n))))      # 3  n is used, and the loop goes back to 0
                         # 4  the exit"
    :facts {:next      [[0 1] [1 2] [2 3] [3 0] [0 4]]
            :alloc     [[:m 1] [:n 2]]
            :operand   [[:m :n 2]]
            :immutable [:n]
            :use       [[:m 2] [:n 3]]}
    :check (fn [ns]
             (def ns-live (liveness ns (ownership ns) operand))
             (and (empty? (made-again ns ns-live))
                  (= [[:n 3 0]] (released-from ns (ownership ns) ns-live))))))

  (def rs (fence 'rs :doc "The v taken out has one owner."
    :source "(defn rs [stack c]
  (def log @[])              # 0  log is made
  (while c                   # 1  the loop head, and the test
    (push log (pop stack)))  # 2  v is taken out of stack   3  v is stored into log
  (length log))              # 4  log is used               5  the exit"
    :facts {:next   [[0 1] [1 2] [2 3] [3 1] [1 4] [4 5]]
            :alloc  [[:log 0]]
            :remove [[:v :stack 2]]
            :store  [[:v :log 3]]
            :use    [[:log 3] [:v 3] [:log 4]]}
    :check (fn [rs] (empty? (in-two-states rs)))))

  {:in-two-states in-two-states :fences [ms ns rs]})
```
