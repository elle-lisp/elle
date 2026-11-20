# Liveness in Datalog

<!-- audited: 2026-11-20 -->

The rules that keep a value live until its last use, and the waits that keep it live longer, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts and the rules of who owns each value,
and [the ownership rules](ownership.md) state each rule in words.
[Release](release.md) releases a value on the edge where it stops being live.
The fences here include the first document, so they run its rules too.

| Derived | Holds when |
|---------|------------|
| `live(v, p)` | a step from `p` on uses `v`, or a value that `v` holds |
| `carries(x, y, p)` | at step `p`, the block parameter `x` holds the value a jump handed it as `y` |
| `live-as(v, p)` | `v` is live at step `p`, or a value that carries `v` is |

## The rules

| Rule | Datalog |
|------|---------|
| Unheld value | `live(v, p) :- use(v, p).` |
| Unheld value | `live(v, p) :- next(p, q), live(v, q), not defined(v, p).` where `defined` is `alloc`, `read`, `remove` or `pass` |
| Recur | `live(y, p) :- pass(y, x, p).` |
| Container waits | `live(c, p) :- live-as(v, p), owned(v, c, p).` where `c` is a container |
| Allocation operand | `live(x, p) :- share(x, v, a), live-as(v, p).` |
| Waits follow a hand-over | `carries(x, y, q) :- pass(y, x, p), next(p, q).` |
| Waits follow a hand-over | `carries(x, y, q) :- carries(x, y, p), next(p, q).` |
| Waits follow a hand-over | `live-as(v, p) :- live(v, p).` |
| Waits follow a hand-over | `live-as(y, p) :- carries(x, y, p), live-as(x, p).` |

A container stays live while a value it holds is live, and the recursion of
`live` carries that up the chain of owners.

A rule that adds a wait adds a clause for `live`, so it must run inside the
same fixpoint. `liveness` therefore takes each such rule as a function. The
function adds its variables to the iteration, and returns what to run in each
round.

In the function below, step 5 is the last step that uses `a` itself. The value
`f` is read out of `e`, and `e` out of `a`, so `a` stays live until the use of
`f` at step 7:

```lisp
(include-file "datalog.md")

(defn defined-in [facts]
  "defined(v, p) as a relation of [v p]: alloc, read, remove or pass."
  (proof:relation (concat (get facts :alloc []) (map (fn [[v c p]] [v p]) (get facts :read []))
                          (map (fn [[v c p]] [v p]) (get facts :remove []))
                          (map (fn [[y x p]] [x p]) (get facts :pass [])))))

(defn carried [facts]
  "carries(x, y, p) as a relation of [[x p] y], under the Waits follow a hand-over rule."
  (def next-r (proof:relation (get facts :next)))
  (def it (proof:iteration))
  (def carries (proof:variable it "carries"))   # [p [x y]]
  # carries(x, y, q) :- pass(y, x, p), next(p, q).
  (proof:insert carries (proof:join (proof:relation (map (fn [[y x p]] [p [x y]]) (get facts :pass [])))
                                    next-r (fn [p xy q] [q xy])))
  (while (proof:changed? it)
    # carries(x, y, q) :- carries(x, y, p), next(p, q).
    (proof:from-join carries carries next-r (fn [p xy q] [q xy])))
  (proof:relation (map (fn [[p [x y]]] [[x p] y]) (proof:elements (proof:complete carries)))))

(defn liveness [facts owned & waits]
  "live(v, p) as a relation of [v p], under the Unheld value, Container waits, Recur and
   Waits follow a hand-over rules, and the rules each of WAITS adds."
  (def [next use pass] (map (fn [k] (get facts k [])) [:next :use :pass]))
  (def by-target (proof:relation (map (fn [[p q]] [q p]) next)))
  (def defined (defined-in facts))
  (def held-by (proof:relation (map (fn [[p [v o]]] [[v p] o])
                                    (filter (fn [[p [v o]]] (not= o :fiber)) owned))))
  (def carrier (carried facts))
  (def it (proof:iteration))
  (def live (proof:variable it "live"))                 # live(v, p) as [p v]
  (def live-at (proof:variable it "live-at"))           # live-as(v, p) as [[v p] true]
  (def live-before (proof:variable it "live-before"))   # [[v p] true]
  (def live-by (proof:variable it "live-by"))           # live-as(v, p) as [v p]
  (def rounds (map (fn [w] (w facts owned it live live-by live-at)) waits))
  # live(y, p) :- pass(y, x, p).
  (proof:extend live (map (fn [[v p]] [p v]) (concat use (map (fn [[y x p]] [y p]) pass))))
  (while (proof:changed? it)
    # live(v, p) :- next(p, q), live(v, q), not defined(v, p).
    (proof:from-join live-before live by-target (fn [q v p] [[v p] true]))
    (proof:from-antijoin live live-before defined (fn [[v p] _] [p v]))
    # live-as(v, p) :- live(v, p).  live-as(y, p) :- carries(x, y, p), live-as(x, p).
    (proof:from-map live-at live (fn [[p v]] [[v p] true]))
    (proof:from-join live-at live-at carrier (fn [[x p] _ y] [[y p] true]))
    # live(c, p) :- live-as(v, p), owned(v, c, p).
    (proof:from-join live live-at held-by (fn [[v p] _ c] [p c]))
    (proof:from-map live-by live-at (fn [[[v p] _]] [v p]))
    (each round in rounds (round)))
  (proof:relation (map (fn [[p v]] [v p]) (proof:elements (proof:complete live)))))

(defn live-steps [live-r v]
  "The steps at which V is live in LIVE-R, the relation liveness returns."
  (sort (map (fn [[w p]] p) (filter (fn [[w p]] (= w v)) (proof:elements live-r)))))

# (defn h []
#   (def a @[])          # 0  a is made
#   (def b @[])          # 1  b is made
#   (def c (array 1))    # 2  c is made
#   (push b c)           # 3  c is stored into b
#   (push a b)           # 4  b is stored into a
#   (def e (get a 0))    # 5  e is read out of a
#   (def f (get e 0))    # 6  f is read out of e
#   (length f))          # 7  f is used
#                        # 8  the exit
(def h {:next  (steps 8)
        :alloc [[:a 0] [:b 1] [:c 2]]
        :read  [[:e :a 5] [:f :e 6]]
        :store [[:c :b 3] [:b :a 4]]
        :use   [[:b 3] [:c 3] [:a 4] [:b 4] [:a 5] [:e 6] [:f 7]]})
(assert (= [1 2 3 4 5 6 7] (live-steps (liveness h (ownership h)) :a))
        "a container stays live until the last use of a value it holds")
```

In the function below, `v` borrows `x`, which `m` holds, so `x` stays live
while `v` is, and `m` stays live until the use of `v` at step 6:

```lisp
(defn operand [facts owned it live live-by live-at]
  "The clause for live under the Allocation operand rule, for liveness."
  (def share-by (proof:relation (map (fn [[x v a]] [v x]) (get (operands facts) 0))))
  (fn []
    # live(x, p) :- share(x, v, a), live(v, p).
    (proof:from-join live live-by share-by (fn [v p x] [p x]))))

# (defn p []
#   (def m @[])          # 0  m is made
#   (def y @[1])         # 1  y is made
#   (push m y)           # 2  y is stored into m
#   (def x (get m 0))    # 3  x is read out of m
#   (def v (pair x ()))  # 4  v is made, with x as an operand
#   (length m)           # 5  m is used
#   (length (first v)))  # 6  v is used
#                        # 7  the exit
(def p {:next      (steps 7)
        :alloc     [[:m 0] [:y 1] [:v 4]]
        :read      [[:x :m 3]]
        :store     [[:y :m 2]]
        :operand   [[:x :v 4]]
        :immutable [:v]
        :use       [[:m 2] [:y 2] [:m 3] [:x 4] [:m 5] [:v 6]]})
(assert (= [1 2 3 4 5 6] (live-steps (liveness p (ownership p) operand) :m))
        "an immutable value keeps the owner of its operand live")
```

A jump hands a value to a block parameter at the value's last use, so the value
itself is dead after the jump, and a wait on it would end there. The parameter
carries the value on: `carries` follows each path from the jump, and `live-as`
holds the value while a parameter that carries it is live. The carry needs no
stop where the parameter is made again, because every jump to the parameter
enters its block at the same step, which follows the first jump too.
`liveness` hands each wait `live-as` as `live-by` and `live-at`, so every wait
reads `live-as` for the value it waits on.

In the function below, the `then` arm hands `v` to `r`, and `v` borrows `x`,
which `m` holds. So `m` stays live until the use of `r` at step 7:

```lisp
# (defn hw [c]
#   (def m @[@[1]])            # 0  m is made
#   (def x (get m 0))          # 1  x is read out of m
#   (def r (if c               # 2  the test
#            (pair x ())       # 3  v is made, with x as an operand   4  v is handed to r
#            (array)))         # 5  u is made                         6  u is handed to r
#   (length r))                # 7  r is used
#                              # 8  the exit
(def hw {:next      [[0 1] [1 2] [2 3] [3 4] [4 7] [2 5] [5 6] [6 7] [7 8]]
         :alloc     [[:m 0] [:v 3] [:u 5]]
         :read      [[:x :m 1]]
         :operand   [[:x :v 3]]
         :immutable [:v]
         :pass      [[:v :r 4] [:u :r 6]]
         :use       [[:m 1] [:x 3] [:r 7]]})
(assert (= [1 2 3 4 5 6 7] (live-steps (liveness hw (ownership hw) operand) :m))
        "a wait on a value holds while the parameter it was handed to is live")
```

The carry does not stop where the value's own name is made again. In `lx`
below, each pass makes `y` and hands it to `x`. The next pass makes `y` again,
while `x` still holds the old one, which borrows `h` out of `m`. So `m` stays
live until the use of `x` at step 8:

```lisp
# (defn lx [c]
#   (def m @[@[1]])              # 0  m is made
#   (loop [x (pair 0 ())]        # 1  i is made   2  i is handed to x   3  the loop head
#     (def h (get m 0))          # 4  h is read out of m
#     (def y (pair h ()))        # 5  y is made, with h as an operand
#     (if c (recur y)            # 6  the test    7  y is handed to x
#         (length (first x)))))  # 8  x is used   9  the exit
(def lx {:next      [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 7] [7 3] [6 8] [8 9]]
         :alloc     [[:m 0] [:i 1] [:y 5]]
         :read      [[:h :m 4]]
         :operand   [[:h :y 5]]
         :immutable [:i :y]
         :pass      [[:i :x 2] [:y :x 7]]
         :use       [[:m 4] [:h 5] [:x 8]]})
(assert (= [1 2 3 4 5 6 7 8] (live-steps (liveness lx (ownership lx) operand) :m))
        "a parameter carries the old value past the step that makes its name again")
```
