# Owner not passed, in Datalog

<!-- audited: 2026-11-21 -->

How a call passes the owner of what it lends at that owner's last use, so the callee releases it, as Datalog that runs on proof.

[Sink](sink.md) lets a call sink an argument, and its fences are included
here. [Signatures](signature.md) holds the Owner not passed rule in words, and
[the region IR](ir.md) says what an instance is.

| Fact | Holds when |
|------|------------|
| `owner(k)` | the instance takes an extra argument `[:owner k]` |
| `owner-of(j, k)` | the instance's parameter `j` borrows from its extra argument `[:owner k]` |

| Derived | Holds when |
|---------|------------|
| `holds(x, j, p)` | the argument `j` that the call at `p` lends borrows from `x` |
| `extra(x, p)` | the call at `p` passes `x` as the owner of what it lends |

## The rules

| Rule | Datalog |
|------|---------|
| Owner not passed | `holds(x, j, p) :- call(r, f, p), arg(z, j, p), lends(f, j), live(x, [:on j p]).` in a probe pass with `next(p, [:on j p]) :- arg(z, j, p).` and `use(z, [:on j p]) :- arg(z, j, p).` |
| Owner not passed | `extra(x, p) :- holds(x, j, p), owned(x, :fiber, p), next(p, q), not live(x, q), not passed(x, p).` where `passed(x, p) :- arg(x, i, p).` |
| Owner not passed | `pass(x, [:owner x p], p) :- extra(x, p).` |
| Owner not passed | `alloc([:owner k], :entry) :- owner(k).` |
| Owner not passed | `live([:owner k], q) :- owner-of(j, k), param(z, j), live-as(z, q).` |

An argument borrows from `x` where `x` would stay live if that argument alone
were used after the call. The probe step `[:on j p]` adds that use, so every
wait rule decides it: Container waits for a read, Immutable capture for a
closure, and Fresh result that borrows for a fresh result.

The pass ends the caller's hold, as a sink does, and nothing reads
`[:owner x p]`. The call names its instance with `owner(k)` for each value it
passes so, and `owner-of(j, k)` for each argument `j` that holds it. The
instance makes `[:owner k]` at `:entry`, as it makes a sunk parameter, and
`[:owner k]` waits for each such `j`. So the Unheld value rule releases it at
their last use, or the instance passes it on.

Below, the call at step 3 lends `h` and `t`, both read out of `xs`, and is the
last use of `xs`. So it passes `xs`, and the instance of `two` releases it
after the use of `u` and `v`:

```lisp
(include-file "sink.md")

(defn holds-of [facts lends & waits]
  "holds(x, j, p) as [x j p], read from a probe pass with WAITS."
  (def args (get facts :arg []))
  # next(p, [:on j p]) :- arg(z, j, p).  use(z, [:on j p]) :- arg(z, j, p).
  (def probe (merge facts {:next (concat (get facts :next) (map (fn [[z j p]] [p [:on j p]]) args))
                           :use  (concat (get facts :use []) (map (fn [[z j p]] [z [:on j p]]) args))}))
  (def live-probe (live-all probe ;waits))
  # call(r, f, p), arg(z, j, p), lends(f, j)
  (def lent-at (proof:join (call-args facts) (proof:relation (map (fn [fi] [fi true]) lends))
                           (fn [[f j] [z r p] _] [[:on j p] [j p]])))
  # holds(x, j, p) :- call(r, f, p), arg(z, j, p), lends(f, j), live(x, [:on j p]).
  (proof:elements (proof:join (proof:map live-probe (fn [[x q]] [q x])) lent-at
                              (fn [_ x [j p]] [x j p]))))

(defn extras [facts lends & waits]
  "extra(x, p) as [x p], read from a first pass and a probe pass with WAITS."
  (def owned (ownership facts))
  (def fiber (proof:map (fiber-held owned) (fn [vp] [vp true])))
  (def live-r (live-all facts ;waits))
  (def passed (proof:relation (map (fn [[x i p]] [x p]) (get facts :arg []))))
  # holds(x, j, p), owned(x, :fiber, p), not passed(x, p)
  (def mine (proof:antijoin (proof:join (proof:relation (map (fn [[x j p]] [[x p] j])
                                                             (holds-of facts lends ;waits)))
                                        fiber (fn [xp j _] [xp true]))
                            passed (fn [[x p] _] [p x])))
  # next(p, q), not live(x, q)
  (def at-next (proof:join mine (proof:relation (get facts :next)) (fn [p x q] [[x q] p])))
  (proof:elements (proof:antijoin at-next live-r (fn [[x q] p] [x p]))))

(defn passed-on [facts lends & waits]
  "FACTS with the late captures as stores, and a pass for each sink and owner not passed."
  (def all (with-captures facts ;waits))
  (def sunk (sinks all lends ;waits))
  # pass(x, [:owner x p], p) :- extra(x, p).
  (put sunk :pass (concat (sunk :pass)
                          (map (fn [[x p]] [x [:owner x p] p]) (extras all lends ;waits)))))

(defn owner-judged [facts lends & waits]
  "The releases of FACTS once each sink and each owner not passed is a pass."
  (released-all (passed-on facts lends ;waits) ;waits))

(defn owning [facts owned-by]
  "FACTS of the instance with owner(k) for each [k js] in OWNED-BY."
  # alloc([:owner k], :entry) :- owner(k).
  (put facts :alloc (concat (get facts :alloc [])
                            (map (fn [[k js]] [[:owner k] :entry]) owned-by))))

(defn waits-on [owned-by]
  "The clause for live under the Owner not passed rule, with owner-of(j, k) for each j in
   JS, per [k js] in OWNED-BY."
  (fn [facts owned it live live-by live-at]
    (def by-param (proof:join (proof:relation (map (fn [[z j]] [j z]) (get facts :param [])))
                              (proof:relation (mapcat (fn [[k js]] (map (fn [j] [j [:owner k]]) js))
                                                      owned-by))
                              (fn [j z o] [z o])))
    (fn []
      # live([:owner k], q) :- owner-of(j, k), param(z, j), live-as(z, q).
      (proof:from-join live live-by by-param (fn [z q o] [q o])))))

# (defn o1 []
#   (def xs (list @[1] @[2]))   # 0  xs is made
#   (def h (first xs))          # 1  h is read out of xs
#   (def t (rest xs))           # 2  t is read out of xs
#   (two h t))                  # 3  h and t are lent, and xs is passed as their owner
#                               # 4  the exit
(def o1 {:next  (steps 4)
         :alloc [[:xs 0]]
         :read  [[:h :xs 1] [:t :xs 2]]
         :call  [[:n :two 3]]
         :arg   [[:h 0 3] [:t 1 3]]
         :use   [[:xs 1] [:xs 2] [:h 3] [:t 3]]})
(assert (= [[:xs 0 3] [:xs 1 3]] (filter (fn [[x]] (= x :xs)) (holds-of o1 lends)))
        "both arguments borrow from xs")
(assert (= [[:xs [:owner :xs 3] 3]] ((passed-on o1 lends) :pass)) "the call passes xs")
(assert (empty? (owner-judged o1 lends)) "so o1 releases nothing itself")

(def tw (returning (owning (instance two []) [[0 [0 1]]])))
(assert (= [[[:owner 0] 0 1]] (released-all tw (waits-on [[0 [0 1]]])))
        "the instance of two releases xs after the last use of u and v")
```

A call passes no owner where the value is used after it, where the fiber does
not hold it, or where the call passes it itself, as the Sink rule decides.
`keep-first` has no `lends` fact, so in `o6` below, the result of `keep-first`
keeps `xs` live, and `o6` releases `xs`:

```lisp
(def o1b (put o1 :use [[:xs 1] [:xs 2] [:h 3] [:t 3] [:xs 4]]))   # (length xs) at 4
(assert (empty? (extras o1b lends)) "a value used after the call is not passed")
# (defn o4 [xs] (two (first xs) (rest xs)))
(def o4 (merge o1 {:alloc [] :param [[:xs 0]]}))
(assert (empty? (extras o4 lends)) "a parameter is not the fiber's to pass")
# (defn o5 [] (def xs (list @[1] @[2])) (def h (first xs)) (two xs h))
(def o5 (merge o1 {:read [[:h :xs 1]] :arg [[:xs 0 3] [:h 1 3]]
                   :use [[:xs 1] [:xs 3] [:h 3]]}))
(assert (empty? (extras o5 lends)) "a value passed itself is not passed again as an owner")

# (defn o6 []
#   (def xs (list @[1] @[2]))   # 0  xs is made
#   (def h (first xs))          # 1  h is read out of xs
#   (keep-first h)              # 2  h is lent, and the result is dropped
#   nil)                        # 3  the exit
(def o6 (calls (results {:next  (steps 3)
                         :alloc [[:xs 0]]
                         :read  [[:h :xs 1]]
                         :call  [[:r :keep-first 2]]
                         :arg   [[:h 0 2]]
                         :use   [[:xs 1] [:h 2]]} ksigs)
               ksigs (lent ksigs)))
(assert (empty? (extras o6 lends (lent ksigs)))
        "keep-first has no lends fact, so xs is not passed")
(assert (= [[:xs 2 3] [:r 2 3]] (owner-judged o6 lends (lent ksigs)))
        "and o6 releases xs and the result after the call")
```

In `o2` below, the closure `k` captures the immutable `xs`, so `xs` waits for
`k`, and the call sinks `k` and passes `xs`. In `o3`, the result of
`keep-first` borrows from `xs`, and the call sinks it and passes `xs`. `call-it`
calls its parameter, which is a target ([signatures](signature.md)); its facts
here say only that it uses `k`:

```lisp
# (defn call-it [k] (k) nil)    # 0  k is used   1  nil is returned   2  the exit
(def call-it {:next [[0 1] [1 2]] :param [[:k 0]] :use [[:k 0]]})
(def clends (concat lends (lends-of :call-it call-it {})))
# (defn o2 []
#   (def xs (list @[1] @[2]))   # 0  xs is made
#   (def k (fn [] (length xs))) # 1  k is made, and captures xs
#   (call-it k))                # 2  k is sunk, and xs is passed as its owner
#                               # 3  the exit
(def o2 {:next      (steps 3)
         :alloc     [[:xs 0] [:k 1]]
         :capture   [[:xs :k 1]]
         :immutable [:xs]
         :call      [[:n :call-it 2]]
         :arg       [[:k 0 2]]
         :use       [[:k 2]]})
(assert (= [[:k [:sink 0 2] 2] [:xs [:owner :xs 2] 2]] ((passed-on o2 clends) :pass))
        "the closure is sunk, and xs is passed as its owner")
(assert (empty? (owner-judged o2 clends)) "so o2 releases nothing itself")

# (defn o3 []
#   (def xs (list @[1] @[2]))   # 0  xs is made
#   (def r (keep-first xs))     # 1  r is made, and borrows from xs
#   (count-up r))               # 2  r is sunk, and xs is passed as its owner
#                               # 3  the exit
(def o3 (calls (results {:next  (steps 3)
                         :alloc [[:xs 0]]
                         :call  [[:r :keep-first 1] [:n :count-up 2]]
                         :arg   [[:xs 0 1] [:r 0 2]]
                         :use   [[:xs 1] [:r 2]]} ksigs)
               ksigs (lent ksigs)))
(assert (= [[:r [:sink 0 2] 2] [:xs [:owner :xs 2] 2]] ((passed-on o3 lends (lent ksigs)) :pass))
        "the result is sunk, and xs is passed as its owner")
(assert (empty? (owner-judged o3 lends (lent ksigs))) "so o3 releases nothing itself")
```

The rule covers every call, and a tail call forces it: a release in the caller
would come after the call, so the call would no longer be a tail call. Below,
the sunk instance of `count-list` passes `lst` as the owner of `t`. The
instance with an owner passes it on to itself, so the recursion keeps its tail
calls and releases the list where it ends:

```lisp
# (defn count-list [lst acc]
#   (if (empty? lst)            # 0  lst is used
#     acc                       # 1  acc is returned
#     (count-list (rest lst)    # 2  t is read out of lst
#                 (+ acc 1))))  # 3  t is lent   4  n is returned
#                               # 5  the exit
(def count-list {:next   [[0 1] [0 2] [1 5] [2 3] [3 4] [4 5]]
                 :param  [[:lst 0] [:acc 1]]
                 :read   [[:t :lst 2]]
                 :call   [[:n :count-list 3]]
                 :arg    [[:t 0 3] [:a 1 3]]
                 :use    [[:lst 0] [:lst 2] [:t 3]]
                 :return [[:acc 1] [:n 4]]})
(def llends (lends-of :count-list count-list {:returned [[:count-list 1]]}))
(def cl-sunk (returning (instance count-list [0])))
(assert (= [[:lst [:owner :lst 3] 3]] ((passed-on cl-sunk llends) :pass))
        "the sunk instance passes lst as the owner of t")
(assert (= [[:lst 0 1]] (owner-judged cl-sunk llends)) "and releases it only where the list ends")

(def cl-owner (returning (owning (instance count-list []) [[0 [0]]])))
(def on0 (waits-on [[0 [0]]]))
(assert (= [[[:owner 0] [:owner [:owner 0] 3] 3]] ((passed-on cl-owner llends on0) :pass))
        "the instance with an owner passes it on, so it calls itself")
(assert (= [[[:owner 0] 0 1]] (owner-judged cl-owner llends on0))
        "and releases the owner where the list ends")
```
