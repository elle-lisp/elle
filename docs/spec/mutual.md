# Mutual groups, in Datalog

<!-- audited: 2026-11-22 -->

How two functions that call each other derive their stored facts in steps, worked through four groups, as Datalog that runs on proof.

[Steps](steps.md) holds the Group stores rule and the steps that derive a
group's stored facts, and [place](place.md) the One place target. The fence of
a group bears its first member's name and holds each member's program.

`h2` stores `x` into `a` on its base case and otherwise calls `k2`, which
calls `h2` back. `k2` has no base case, so the first step's unnamed places keep
both facts. In `h`, the path that calls `k` returns a new array and never
stores `x`, so neither stores, and both results are fresh.

`f2` stores `x` into `a` on its base case and `g2` into `b` on its own, and
each calls the other. Each step keeps the first place, and the last step derives
both, so the One place target refuses the two.

`k3` returns `x` after its call into `h3`, which stores `x`, so `k3` does not
return `x` itself. Rounds that read no stored facts give `k3` that `itself`
fact. The call in `h3` then hands `x` to its result as it stores `x`, the fiber
holds that result at the return, and the steps keep neither fact:

```lisp
(fn []
  (def {:fence fence} ((import "../spec.md")))
  (def {:one-place one-place} ((import "./place.md")))
  (def {:rounds rounds} ((import "./recursion.md")))
  (def {:group-stores group-stores} ((import "./steps.md")))

  (def k2 {:next [[0 1] [1 2]] :param [[:a 0] [:x 1] [:n 2]] :call [[:m :h2 0]]
           :arg [[:a 0 0] [:x 1 0] [:n 2 0]] :use [[:a 0] [:x 0]] :return [[:m 1]]})
  (def h2 (fence 'h2 :doc "h2 and k2 store x into a."
    :source "(defn h2 [a x n]
  (if (= n 0)                 # 0  the test
    (begin (push a x) nil)    # 1  x is stored into a   2  z is returned
    (k2 a x n)))              # 3  m is the call's result   4  m is returned   5  the exit
(defn k2 [a x n] (h2 a x n))  # 0  m is the call's result   1  m is returned   2  the exit"
    :facts {:next [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5]] :param [[:a 0] [:x 1] [:n 2]]
            :store [[:x :a 1]] :call [[:m :k2 3]] :arg [[:a 0 3] [:x 1 3] [:n 2 3]]
            :use [[:n 0] [:a 1] [:x 1] [:a 3] [:x 3]] :return [[:z 2] [:m 4]]}
    :check (fn [h2] (= [[:h2 1 [:param 0]] [:k2 1 [:param 0]]]
                       (get (group-stores [[:h2 h2] [:k2 k2]]) 0)))))

  (def h (fence 'h :doc "x is stored on one path of h alone, so v is lent, and h and k are fresh."
    :source "(defn h [a x n]
  (if (= n 0)                 # 0  the test
    (begin (push a x) nil)    # 1  x is stored into a   2  z is returned
    (begin (def v @[])        # 3  v is made
           (k a v n) v)))     # 4  v is passed to k   5  v is returned   6  the exit
(defn k [a x n] (h a x n))"
    :facts {:next [[0 1] [1 2] [2 6] [0 3] [3 4] [4 5] [5 6]] :param [[:a 0] [:x 1] [:n 2]]
            :store [[:x :a 1]] :alloc [[:v 3]] :call [[:m :k 4]] :arg [[:a 0 4] [:v 1 4] [:n 2 4]]
            :use [[:n 0] [:a 1] [:x 1] [:a 4] [:v 4]] :return [[:z 2] [:v 5]]}
    :check (fn [h]
             (def k (merge k2 {:call [[:m :h 0]]}))
             (def hk (get (group-stores [[:h h] [:k k]]) 0))
             (and (empty? hk) (= [:h :k] ((rounds [[:h h] [:k k]] {:stored hk}) :fresh))))))

  (def f2 (fence 'f2 :doc "Each keeps its first place, and the last step refuses both."
    :source "(defn f2 [a b x n]
  (if (= n 0)                 # 0  the test
    (begin (push a x) nil)    # 1  x is stored into a   2  z is returned
    (g2 a b x n)))            # 3  m is the call's result   4  m is returned   5  the exit
(defn g2 [a b x n] (if (= n 0) (begin (push b x) nil) (f2 a b x n)))"
    :facts {:next [[0 1] [1 2] [2 5] [0 3] [3 4] [4 5]] :param [[:a 0] [:b 1] [:x 2] [:n 3]]
            :store [[:x :a 1]] :call [[:m :g2 3]] :arg [[:a 0 3] [:b 1 3] [:x 2 3] [:n 3 3]]
            :use [[:n 0] [:a 1] [:x 1] [:a 3] [:b 3] [:x 3]] :return [[:z 2] [:m 4]]}
    :check (fn [f2]
             (def g2 (merge f2 {:store [[:x :b 1]] :call [[:m :f2 3]]}))
             (def [stored derived] (group-stores [[:f2 f2] [:g2 g2]]))
             (and (= [[:f2 2 [:param 0]] [:g2 2 [:param 1]]] stored)
                  (= [[:f2 2] [:g2 2]] (one-place derived))))))

  # (defn h3 [a x n] (if (= n 0) (begin (push a x) nil) (k3 a x n)))
  # (defn k3 [a x n]
  #   (h3 a x n)                  # 0  m is the call's result
  #   x)                          # 1  x is returned   2  the exit
  (def h3 (fence 'h3 :doc "h3 and k3 each store x into a."
    :facts (merge h2:facts {:call [[:m :k3 3]]})
    :check (fn [h3]
             (def k3 (merge k2 {:call [[:m :h3 0]] :return [[:x 1]]}))
             (= [[:h3 1 [:param 0]] [:k3 1 [:param 0]]] (get (group-stores [[:h3 h3] [:k3 k3]]) 0)))))

  {:fences [h2 h f2 h3]})
```
