# Sink in Datalog

<!-- audited: 2026-11-19 -->

How a call sinks an argument at its last use, so that the callee's instance releases it, as Datalog that runs on proof.

[Call](call.md) reads a callee's signature for each argument, and
[borrows](borrows.md) names what a result borrows from. The fences here
include the second document. [Signatures](signature.md) holds the Sink rule in
words, and [the region IR](ir.md) says what an instance is.

| Fact | Holds when |
|------|------------|
| `sunk(i)` | the instance takes its parameter `i` sunk |

| Derived | Holds when |
|---------|------------|
| `lends(f, i)` | `f` neither stores nor returns its parameter `i`, and no result of `f` borrows from it |
| `sinks(y, i, p)` | the call at `p` sinks `y`, its argument `i` |
| `aliased(y, i, p)` | another argument of the call at `p` borrows from `y`, its argument `i` |

## The rules

| Rule | Datalog |
|------|---------|
| Sink | `lends(f, i) :- param(x, i), not stores(f, i), not returned(f, i), not itself(f, i), not borrows(f, [:param i]).` where `stores(f, i) :- stored(f, i, t).` |
| Sink | `sinks(y, i, p) :- call(r, f, p), arg(y, i, p), lends(f, i), owned(y, :fiber, p), next(p, q), not live(y, q), not aliased(y, i, p).` |
| Sink | `aliased(y, i, p) :- arg(y, i, p), live(y, [:in i p]).` in a probe pass with `next(p, [:in i p]) :- arg(y, i, p).` and `use(z, [:in i p]) :- arg(y, i, p), arg(z, j, p), i ≠ j.` |
| Sink | `pass(y, [:sink i p], p) :- sinks(y, i, p).` |
| Sink | `alloc(x, :entry) :- sunk(i), param(x, i).` with `next(:entry, 0)` |

A parameter that a result borrows from stays live through the callee's exit,
under the Fresh result that borrows rule. A sunk one would then have a release
in neither function. So "returns" in the rule covers a result that borrows from
the parameter, and the call lends it.

In the instance, the sunk parameter is made at `:entry`, the step before step 0.
The fiber holds it from the start, so the Unheld value rule releases it at its
last use, and the instance may sink it again. Below, the instance of `count-up`
releases `a` after its use. The instance of `keep-first` would release `c`
nowhere, which is why `keep-first` lends:

```lisp
(include-file "borrows.md")

(defn lends-of [f facts sigs]
  "lends(f, i) as [f i], from the parameters in FACTS and the signature facts SIGS."
  # stores(f, i) :- stored(f, i, t).
  (def taken (proof:relation
               (concat (map (fn [[g i t]] [g i]) (get sigs :stored []))
                       (get sigs :returned []) (get sigs :itself [])
                       (map (fn [[g [_ i]]] [g i])
                            (filter (fn [[g [tag]]] (= tag :param)) (get sigs :borrows []))))))
  # lends(f, i) :- param(x, i), not stores(f, i), not returned(f, i), not itself(f, i),
  #                not borrows(f, [:param i]).
  (proof:elements (proof:antijoin (proof:relation (map (fn [[x i]] [[f i] true])
                                                       (get facts :param [])))
                                  taken (fn [fi _] fi))))

(defn instance [facts sunk]
  "The FACTS of the instance that takes each parameter in SUNK sunk."
  # alloc(x, :entry) :- sunk(i), param(x, i).
  (def made (proof:join (proof:relation (map (fn [[x i]] [i x]) (get facts :param [])))
                        (proof:relation (map (fn [i] [i true]) sunk)) (fn [i x _] [x :entry])))
  (merge facts {:alloc (concat (get facts :alloc []) (proof:elements made))
                :next  (concat [[:entry 0]] (get facts :next))}))

# (defn count-up [a]
#   (+ (length a) 1))           # 0  a is used   1  n is returned   2  the exit
(def count-up {:next [[0 1] [1 2]] :param [[:a 0]] :use [[:a 0]] :return [[:n 1]]})
# (defn two [u v]
#   (+ (length u) (length v)))  # 0  u and v are used   1  n is returned   2  the exit
(def two {:next [[0 1] [1 2]] :param [[:u 0] [:v 1]] :use [[:u 0] [:v 0]] :return [[:n 1]]})

(def ksigs {:fresh [:keep-first] :borrows (borrows-of :keep-first keep-first)})
(def lends (concat (lends-of :count-up count-up {}) (lends-of :two two {})
                   (lends-of :keep-first keep-first ksigs)))
(assert (= [[:count-up 0] [:two 0] [:two 1]] lends) "keep-first lends nothing")

(def cu (returning (instance count-up [0])))
(assert (= [[:a 0 1]] (releases cu (ownership cu) removal operand captured))
        "the sunk instance releases a after its last use")
(def kf (returning (instance keep-first [0])))
(assert (empty? (releases kf (ownership kf) removal operand captured))
        "a sunk parameter that the result borrows from is never released")
```

A call sinks a value the fiber holds where nothing uses it after the call. The
pass ends the caller's hold, as a jump does under the Recur rule. Nothing reads
`[:sink i p]`, so the caller holds no release for the value. The call names the
instance of `f` with `sunk(i)` for each argument it sinks.

Another argument borrows from `y` where `y` would stay live if that argument
were used after the call. The probe pass adds that use at the step `[:in i p]`,
so every wait rule decides it, and the call then lends `y`.

In `s1` below, nothing uses `a` after the call at step 2, so the call sinks it,
and `count-up` releases it. `b` is used after the call at step 3, so that call
lends it. In `s2`, `h` is read out of `x`, so the call lends both:

```lisp
(defn sinks [facts lends & waits]
  "FACTS with a pass to [:sink i p] for each argument y that a call sinks, read from
   a first pass and a probe pass with WAITS."
  (def at (proof:join (proof:relation (map (fn [[r f p]] [p f]) (get facts :call [])))
                      (proof:relation (map (fn [[y i p]] [p [y i]]) (get facts :arg [])))
                      (fn [p f [y i]] [[f i] [y p]])))
  (def owned (ownership facts))
  (def fiber (proof:relation (map (fn [[p [v o]]] [[v p] true])
                                  (filter (fn [[p [v o]]] (= o :fiber)) owned))))
  # call(r, f, p), arg(y, i, p), lends(f, i), owned(y, :fiber, p)
  (def mine (proof:join (proof:join at (proof:relation (map (fn [fi] [fi true]) lends))
                                    (fn [_ [y p] _] [[y p] true]))
                        fiber (fn [[y p] _ _] [[y p] true])))
  (def mine-i (proof:join (proof:relation (map (fn [[y i p]] [[y p] i]) (get facts :arg [])))
                          mine (fn [[y p] i _] [p [y i]])))
  # next(p, [:in i p]), and use(z, [:in i p]) for each other argument z
  (def args-at (proof:relation (map (fn [[y i p]] [p [y i]]) (get facts :arg []))))
  (def others (filter (fn [[p [y i] [z j]]] (not= i j))
                      (proof:elements (proof:join args-at args-at (fn [p a b] [p a b])))))
  (def probe (merge facts {:next (concat (get facts :next)
                                         (map (fn [[y i p]] [p [:in i p]]) (get facts :arg [])))
                           :use  (concat (get facts :use [])
                                         (map (fn [[p [y i] [z j]]] [z [:in i p]]) others))}))
  (def live-r (liveness facts owned removal operand captured ;waits))
  (def live-probe (liveness probe (ownership probe) removal operand captured ;waits))
  # next(p, q), not live(y, q)
  (def last (proof:antijoin (proof:join mine-i (proof:relation (get facts :next))
                                        (fn [p [y i] q] [[y q] [i p]]))
                            live-r (fn [[y q] [i p]] [[y [:in i p]] [i p]])))
  # not aliased(y, i, p), and pass(y, [:sink i p], p) :- sinks(y, i, p).
  (def sunk (proof:antijoin last live-probe (fn [[y _] [i p]] [y [:sink i p] p])))
  (put facts :pass (concat (get facts :pass []) (proof:elements sunk))))

(defn sunk-judged [facts lends & waits]
  "The releases of FACTS once the late captures are stores and each sink is a pass."
  (def all (sinks (with-captures facts ;waits) lends ;waits))
  (releases all (ownership all) removal operand captured ;waits))

# (defn s1 []
#   (def a (array 1 2))         # 0  a is made
#   (def b (array 3))           # 1  b is made
#   (count-up a)                # 2  a is sunk
#   (count-up b)                # 3  b is lent
#   (length b))                 # 4  b is used
#                               # 5  the exit
(def s1 {:next  (map (fn [p] [p (+ p 1)]) (range 5))
         :alloc [[:a 0] [:b 1]]
         :call  [[:n2 :count-up 2] [:n3 :count-up 3]]
         :arg   [[:a 0 2] [:b 0 3]]
         :use   [[:a 2] [:b 3] [:b 4]]})
(assert (= [[:a [:sink 0 2] 2]] ((sinks s1 lends) :pass)) "the call at 2 sinks a")
(assert (= [[:b 4 5]] (sunk-judged s1 lends)) "a is released in count-up")

# (defn s2 []
#   (def x (list @[1] @[2]))    # 0  x is made
#   (def h (first x))           # 1  h is read out of x
#   (two x h))                  # 2  x and h are lent to two
#                               # 3  the exit
(def s2 {:next  (map (fn [p] [p (+ p 1)]) (range 3))
         :alloc [[:x 0]]
         :read  [[:h :x 1]]
         :call  [[:n :two 2]]
         :arg   [[:x 0 2] [:h 1 2]]
         :use   [[:x 1] [:x 2] [:h 2]]})
(assert (empty? ((sinks s2 lends) :pass)) "neither x nor h is sunk")
(assert (= [[:x 2 3]] (sunk-judged s2 lends)) "x is lent, since h borrows from it")
```

The sink reads its liveness after [calls](call.md) and after the late captures
of [captures](capture.md). In `s3` below, `k` is used after the last use of
`a`, so the capture is late and moves `a` into `k`, and the call lends `a`.
Read in the other order, the call would sink `a`, and `count-up` would release
it before `k` runs. `onward` is a sunk instance that sinks its parameter again:

```lisp
# (defn s3 []
#   (def a @[1])                # 0  a is made
#   (def k (fn [] (length a)))  # 1  k is made, and captures a
#   (count-up a)                # 2  a is lent, since k holds it
#   (k))                        # 3  k is used
#                               # 4  the exit
(def s3 {:next    (map (fn [p] [p (+ p 1)]) (range 4))
         :alloc   [[:a 0] [:k 1]]
         :capture [[:a :k 1]]
         :call    [[:n :count-up 2]]
         :arg     [[:a 0 2]]
         :use     [[:a 2] [:k 3]]})
(def s3x (sinks (with-captures s3) lends))
(assert (= [[:a :k 1]] (s3x :store)) "the late capture moves a into k")
(assert (empty? (s3x :pass)) "so the call lends a")
(assert (= [[:k 3 4]] (sunk-judged s3 lends)) "and a dies with k")

# (defn onward [a]
#   (count-up a))               # 0  a is sunk   1  n is returned   2  the exit
(def onward (returning (instance {:next   [[0 1] [1 2]]
                                  :param  [[:a 0]]
                                  :call   [[:n :count-up 0]]
                                  :arg    [[:a 0 0]]
                                  :use    [[:a 0]]
                                  :return [[:n 1]]} [0])))
(assert (= [[:a [:sink 0 0] 0]] ((sinks onward lends) :pass)) "a sunk parameter sinks again")
(assert (empty? (sunk-judged onward lends)) "so onward releases nothing itself")
```

## Not written yet

- The Aliased arguments rule, which sinks an argument that another argument of
  the same call borrows from.
- The Owner not passed rule.
