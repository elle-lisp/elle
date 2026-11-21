# Calls in Datalog

<!-- audited: 2026-11-21 -->

How a call reads its callee's signature to lend, store or hand over each argument, as Datalog that runs on proof.

[Datalog](datalog.md) holds the facts of a step, [capture](capture.md) the
rules the fences here include, and [signatures](signature.md) the Call and
Returned itself rules in words.

| Fact | Holds when |
|------|------------|
| `call(r, f, p)` | step `p` calls `f`, and `r` receives its result |
| `arg(y, i, p)` | step `p` passes the value `y` as the argument `i` |
| `stored(f, i, t)` | `f` stores its parameter `i` into the place `t`, as [stored](stored.md) derives it |
| `returned(f, i)` | the result of `f` is borrowed from its parameter `i` |
| `itself(f, i)` | the result of `f` is its parameter `i` itself, on every path, as [itself](itself.md) derives it |

A place is `[:param j]`, the argument `j`; `:result`, the fresh result;
`[:free n]`, the free name `n`; or `:closure`, the callee itself
([free](free.md)). A free name has no owner fact, as a parameter has none.

| Derived | Holds when |
|---------|------------|
| `kept-on(r, y, p)` | in the first pass, `y` is live after the call at `p` that returns it itself |
| `handed(r, y, p)` | the call at `p` hands `y` to its result `r` |

## The rules

| Rule | Datalog |
|------|---------|
| Call | `store(y, z, p) :- call(r, f, p), arg(y, i, p), stored(f, i, [:param j]), arg(z, j, p).` |
| Call | `store(y, r, p) :- call(r, f, p), arg(y, i, p), stored(f, i, :result).` |
| Call | `store(y, n, p) :- call(r, f, p), arg(y, i, p), stored(f, i, [:free n]).` |
| Call | `read(r, y, p) :- call(r, f, p), arg(y, i, p), returned(f, i).` |
| Returned itself | `kept-on(r, y, p) :- call(r, f, p), arg(y, i, p), itself(f, i), next(p, q), live(y, q).` |
| Returned itself | `handed(r, y, p) :- call(r, f, p), arg(y, i, p), itself(f, i), not kept-on(r, y, p).` |
| Returned itself | `pass(y, r, p) :- handed(r, y, p).` |
| Returned itself | `read(r, y, p) :- kept-on(r, y, p).` |

A borrowed parameter lends the value for the call, so the call gives `use(y,
p)` and needs no clause. A returned result is held by its argument, as a read
is, so the argument waits for it and it has no release of its own.

A result that is its argument itself is the same value. Where nothing uses
the argument after the call, every later use goes through the result, so the
hand-over renames the value, as a jump does under the Recur rule. The first
pass ties no such result to its argument, and `kept-on` reads its `live`. The
call facts are derived before the capture pass.

A callee that may return either of two parameters, or that stores the one it
returns, has no `itself` fact. Two hand-overs would leave one region with no
release, and a stored value is not the fiber's to hand over.

Below, `h` is the function in [release](release.md) with each `push` written as
a call, and its release is unchanged. In `m`, `b` moves into `w`, `w` into
`a`, and `d` into `log`, so only `a` is released, after the use of `r`:

```lisp
(include-file "capture.md")

(defn call-args [facts]
  "call(r, f, p), arg(y, i, p) as a relation of [[f i] [y r p]], from FACTS."
  (proof:join (proof:relation (map (fn [[r f p]] [p [r f]]) (get facts :call [])))
              (proof:relation (map (fn [[y i p]] [p [y i]]) (get facts :arg [])))
              (fn [p [r f] [y i]] [[f i] [y r p]])))

(defn tag? [tag t]
  "Whether the place T is [TAG ...], as [:param j] and [:free n] are; :result and :closure are not."
  (and (array? t) (= (get t 0) tag)))

(defn call-facts [facts sigs handed]
  "FACTS with the stores, reads and passes the Call and Returned itself rules derive,
   where HANDED holds each [r y p] a call hands over."
  (def at (call-args facts))
  (defn by-sig [k] (proof:join at (proof:relation (map (fn [fi] [fi true]) (get sigs k [])))
                             (fn [_ [y r p] _] [r y p])))
  # read(r, y, p) :- returned.  read(r, y, p) :- kept-on(r, y, p).
  (def reads (proof:merge (by-sig :returned)
                          (proof:antijoin (proof:map (by-sig :itself) (fn [ryp] [ryp true]))
                                          (proof:relation handed) (fn [ryp _] ryp))))
  (def to (proof:elements (proof:join at (proof:relation (map (fn [[f i t]] [[f i] t])
                                                              (get sigs :stored [])))
                                      (fn [[f _] [y r p] t] [t y r p f]))))
  (defn tagged [tag] (filter (fn [[t]] (tag? tag t)) to))
  (def into-param (proof:join (proof:relation (map (fn [[[_ j] y r p]] [[p j] y]) (tagged :param)))
                              (proof:relation (map (fn [[y i p]] [[p i] y]) (get facts :arg [])))
                              (fn [[p j] y z] [y z p])))
  (merge facts
         {:read  (concat (get facts :read []) (proof:elements reads))
          :store (concat (get facts :store []) (proof:elements into-param)
                         (map (fn [[t y r p]] [y r p]) (filter (fn [[t]] (= t :result)) to))
                         (map (fn [[[_ n] y r p]] [y n p]) (tagged :free))
                         # store(y, f, p) :- call(r, f, p), arg(y, i, p), stored(f, i, :closure).
                         (map (fn [[t y r p f]] [y f p]) (filter (fn [[t]] (= t :closure)) to)))
          # free(n) :- call(r, f, p), arg(y, i, p), stored(f, i, [:free n]).
          :free  (distinct (concat (get facts :free []) (map (fn [[[_ n]]] n) (tagged :free))))
          :pass  (concat (get facts :pass []) (map (fn [[r y p]] [y r p]) handed))}))

(defn calls [facts sigs & waits]
  "FACTS with what each call derives, each hand-over read from a first pass with WAITS."
  (def pass1 (call-facts facts (put sigs :itself []) []))
  (def live1 (proof:map (liveness pass1 (ownership pass1) removal operand ;waits) (fn [vp] [vp true])))
  (def mine (proof:join (call-args facts) (proof:relation (map (fn [fi] [fi true]) (get sigs :itself [])))
                        (fn [_ [y r p] _] [p [r y]])))
  # kept-on(r, y, p) :- call(r, f, p), arg(y, i, p), itself(f, i), next(p, q), live(y, q).
  (def kept-on (proof:join (proof:join mine (proof:relation (get facts :next))
                                       (fn [p [r y] q] [[y q] [r y p]]))
                           live1 (fn [_ ryp _] ryp)))
  # handed(r, y, p) :- call(r, f, p), arg(y, i, p), itself(f, i), not kept-on(r, y, p).
  (def handed (proof:antijoin (proof:map mine (fn [[p [r y]]] [[r y p] true])) kept-on
                              (fn [ryp _] ryp)))
  (call-facts facts sigs (proof:elements handed)))

(def h2 (calls (merge h {:store [] :call [[:r3 :push 3] [:r4 :push 4]]
                         :arg [[:b 0 3] [:c 1 3] [:a 0 4] [:b 1 4]]})
               {:stored [[:push 1 [:param 0]]] :returned [[:push 0]]}))
(assert (= [[:a 7 8]] (releases h2 (ownership h2))) "a push written as a call stores as before")

(def sigs {:stored   [[:wrap 0 :result] [:keep 1 [:param 0]] [:note 0 [:free :log]]
                      [:push 1 [:param 0]]]
           :returned [[:keep 0]]
           :itself   [[:randomize 0] [:push 0]]})

# (defn wrap [x] @[x])               (defn keep [acc x] (push acc x))
# (defn note [x] (push log x))       (defn randomize [g] (put g 0 1) g)
# (defn m []
#   (def a @[])                 # 0  a is made
#   (def b (array 1))           # 1  b is made
#   (def w (wrap b))            # 2  w is made, and b is stored into w
#   (def r (keep a w))          # 3  w is stored into a, and r is read out of a
#   (def d (array 2))           # 4  d is made
#   (note d)                    # 5  d is stored into log
#   (length r))                 # 6  r is used
#                               # 7  the exit
(def m (calls {:next  (steps 7)
               :alloc [[:a 0] [:b 1] [:w 2] [:d 4]]
               :call  [[:w :wrap 2] [:r :keep 3] [:n :note 5]]
               :arg   [[:b 0 2] [:a 0 3] [:w 1 3] [:d 0 5]]
               :use   [[:b 2] [:a 3] [:w 3] [:d 5] [:r 6]]} sigs))
(assert (= [[:a 6 7]] (releases m (ownership m) removal operand)) "each stored argument moves")
```

In `n` below, `x` is borrowed from `coll`, so its store into `a` through `keep`
and into `log` through `note` are both refused, under the Store inside a unit
rule:

```lisp
# (defn n [coll]
#   (def a @[])                 # 0  a is made
#   (def x (get coll 0))        # 1  x is read out of coll
#   (keep a x)                  # 2  x is stored into a
#   (note x))                   # 3  x is stored into log
#                               # 4  the exit
(def n (calls {:next  (steps 4)
               :alloc [[:a 0]]
               :read  [[:x :coll 1]]
               :call  [[:r :keep 2] [:s :note 3]]
               :arg   [[:a 0 2] [:x 1 2] [:x 0 3]]
               :use   [[:a 2] [:x 2] [:x 3]]} sigs))
(assert (= [[:x :log 3] [:x :a 2]] (refusals n (ownership n))) "a borrow cannot be stored")
```

In `reset` below, nothing uses `a` after `randomize` returns it, so the call
hands `a` to `r`, and the fiber moves `r` into `state`. In `o`, the inner
`push` hands `y` over, and `outer` is used after the outer `push`, so `s` is
read out of it. In `keep-a`, `a` is used after the call, so `r` is read out of
`a`, and `a` waits for `r`:

```lisp
# (defn reset [state]
#   (def a (array 0 0))         # 0  a is made
#   (def r (randomize a))       # 1  a is handed to r
#   (put state :grid r))        # 2  r is stored into state
#                               # 3  the exit
(def reset (calls {:next  (steps 3)
                   :alloc [[:a 0]]
                   :call  [[:r :randomize 1]]
                   :arg   [[:a 0 1]]
                   :store [[:r :state 2]]
                   :use   [[:a 1] [:r 2]]} sigs))
(assert (empty? (refusals reset (ownership reset))) "a result handed over is the fiber's to store")
(assert (empty? (releases reset (ownership reset) removal operand)) "and state holds it")

# (defn o []
#   (def outer @[])             # 0  outer is made
#   (def y (array))             # 1  y is made
#   (def r (push y 1))          # 2  y is handed to r
#   (def s (push outer r))      # 3  r is stored into outer, and s is read out of outer
#   (length outer))             # 4  outer is used
#                               # 5  the exit
(def o (calls {:next  (steps 5)
               :alloc [[:outer 0] [:y 1]]
               :call  [[:r :push 2] [:s :push 3]]
               :arg   [[:y 0 2] [:outer 0 3] [:r 1 3]]
               :use   [[:y 2] [:outer 3] [:r 3] [:outer 4]]} sigs))
(assert (empty? (refusals o (ownership o))) "the inner array moves into outer")
(assert (= [[:outer 4 5]] (releases o (ownership o) removal operand)) "outer alone is released")

# (defn keep-a []
#   (def a (array 0 0))         # 0  a is made
#   (def r (randomize a))       # 1  r is read out of a
#   (length a)                  # 2  a is used
#   (length r))                 # 3  r is used
#                               # 4  the exit
(def keep-a (calls {:next  (steps 4)
                    :alloc [[:a 0]]
                    :call  [[:r :randomize 1]]
                    :arg   [[:a 0 1]]
                    :use   [[:a 1] [:a 2] [:r 3]]} sigs))
(assert (= [[:r :a 1]] (keep-a :read)) "an argument used after the call keeps its hold")
(assert (= [[:a 3 4]] (releases keep-a (ownership keep-a) removal operand)) "a waits for r")
```
