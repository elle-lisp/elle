# A fiber's payload, in Datalog

<!-- audited: 2026-11-21 -->

How the fiber value holds the payload a resume leaves, and where a read of it takes it out or borrows it, as Datalog on proof.

[Crossing](crossing.md) moves the payload out of the child, and its fences are
included here. [The ownership rules](ownership.md) state the Payload rule in
words, and [removal](removal.md) holds the waits a removal adds.

| Fact | Holds when |
|------|------------|
| `payload(v, f, p)` | step `p` reads the payload of the fiber value `f` as `v`, as `fiber/resume` and `fiber/value` do |
| `resume(f, p)` | step `p` resumes the fiber value `f`, so `f` holds a new payload after it |

| Derived | Holds when |
|---------|------------|
| `within(f, k)` | the closure `k` captures `f`, itself or through another closure |
| `reads(f, p)` | step `p` may read the payload that `f` holds |
| `wanted(f, p)` | a step from `p` on may read the payload that `f` holds at `p` |
| `takes(v, f, p)` | the read of `v` at step `p` takes the payload out of `f` |

## The rules

| Rule | Datalog |
|------|---------|
| Payload | `within(f, k) :- capture(f, k, a).` and `within(f, j) :- capture(k, j, a), within(f, k).` |
| Payload | `reads(f, p) :- use(f, p), not resume(f, p), not capture(f, k, p).` |
| Payload | `reads(f, p) :- use(k, p), within(f, k).` |
| Payload | `wanted(f, p) :- reads(f, p).` |
| Payload | `wanted(f, p) :- next(p, q), wanted(f, q), not resume(f, p).` |
| Payload | `takes(v, f, p) :- payload(v, f, p), owned(f, :fiber, p), not later(f, p).` where `later(f, p) :- next(p, q), wanted(f, q).` |
| Payload | `remove(v, f, p) :- takes(v, f, p).` |
| Payload | `read(v, f, p) :- payload(v, f, p), not takes(v, f, p).` |
| Payload | `remove([:out p], f, p) :- resume(f, p).` |

The child's emit moves the payload to the fiber value, which holds it until
the next resume replaces it. That resume takes the old payload out, as a
removal does, and where a read took it out already, the release of `[:out p]`
frees nothing. [Replace](replace.md) says what the fiber value holds after the
resume.

A read that takes the payload out gives the fiber a value it owns, and every
other read borrows it from the fiber value, which waits for it. No native
declares whether it reads a payload, so each use of the fiber value counts as a
read, and so does each use of a closure that captures it, but not the step that
makes that closure. A fiber value the fiber does not own may be read after this
function, so its payload is borrowed. The facts are those before the late
captures, as for a call.

In `relay` below, nothing reads the payload of `f` after the resume at step 2,
so `r` takes it out. The emit at step 3 then moves `r` on:

```lisp
(include-file "crossing.md")

(defn payloads [facts]
  "FACTS with each payload read as a removal or a read, and a removal of the old payload at
   each resume, under the Payload rule."
  (def [next capture payload] (map (fn [k] (get facts k [])) [:next :capture :payload]))
  # use(x, a) :- capture(x, k, a).
  (def use (concat (get facts :use []) (map (fn [[x k a]] [x a]) capture)))
  (def resume (proof:relation (get facts :resume [])))
  (def it (proof:iteration))
  (def within (proof:variable it "within"))   # [k f]
  # within(f, k) :- capture(f, k, a).
  (proof:extend within (map (fn [[f k a]] [k f]) capture))
  (while (proof:changed? it)
    # within(f, j) :- capture(k, j, a), within(f, k).
    (proof:from-join within within (proof:relation (map (fn [[k j a]] [k j]) capture))
                     (fn [k f j] [j f])))
  # reads(f, p) :- use(f, p), not resume(f, p), not capture(f, k, p).
  # reads(f, p) :- use(k, p), within(f, k).
  (def quiet (proof:merge resume (proof:relation (map (fn [[x k a]] [x a]) capture))))
  (def reads (concat (proof:elements (proof:antijoin (proof:relation (map (fn [fp] [fp true]) use))
                                                     quiet (fn [fp _] fp)))
                     (proof:elements (proof:join (proof:relation use) (proof:complete within)
                                                 (fn [k p f] [f p])))))
  (def by-target (proof:relation (map (fn [[p q]] [q p]) next)))
  (def it2 (proof:iteration))
  (def wanted (proof:variable it2 "wanted"))   # [p f]
  (def before (proof:variable it2 "before"))   # [[f p] true]
  # wanted(f, p) :- reads(f, p).
  (proof:extend wanted (map (fn [[f p]] [p f]) reads))
  (while (proof:changed? it2)
    # wanted(f, p) :- next(p, q), wanted(f, q), not resume(f, p).
    (proof:from-join before wanted by-target (fn [q f p] [[f p] true]))
    (proof:from-antijoin wanted before resume (fn [[f p] _] [p f])))
  # later(f, p) :- next(p, q), wanted(f, q).
  (def later (proof:join (proof:complete wanted) by-target (fn [q f p] [f p])))
  # takes(v, f, p) :- payload(v, f, p), owned(f, :fiber, p), not later(f, p).
  (def held (proof:join (proof:relation (map (fn [[v f p]] [[f p] v]) payload))
                        (proof:map (fiber-held (ownership facts)) (fn [fp] [fp true]))
                        (fn [fp v _] [fp v])))
  (def takes (proof:elements (proof:antijoin held later (fn [[f p] v] [v f p]))))
  (merge facts
         # remove(v, f, p) :- takes(v, f, p).  remove([:out p], f, p) :- resume(f, p).
         {:remove (concat (get facts :remove []) takes
                          (map (fn [[f p]] [[:out p] f p]) (get facts :resume [])))
          # read(v, f, p) :- payload(v, f, p), not takes(v, f, p).
          :read   (concat (get facts :read [])
                          (proof:elements (proof:antijoin (proof:relation (map (fn [t] [t true]) payload))
                                                          (proof:relation takes) (fn [t _] t))))}))

# (defn relay [g]
#   (def f (fiber/new g |:yield|))   # 0  f is made
#   (def w @[1])                    # 1  w is made
#   (def r (fiber/resume f w))      # 2  w crosses, f is resumed, and r is its payload
#   (yield r)                       # 3  r crosses, and s is made
#   nil)                            # 4  the exit
(def relay (payloads {:next    (steps 4)
                      :alloc   [[:f 0] [:w 1] [:s 3]]
                      :resume  [[:f 2]]
                      :payload [[:r :f 2]]
                      :cross   [[:w 2] [:r 3]]
                      :use     [[:f 2] [:w 2] [:r 3]]}))
(assert (= [[:r :f 2] [[:out 2] :f 2]] (relay :remove)) "r takes the payload out of f")
(assert (empty? (cross-refusals relay)) "so the fiber moves r on")
(assert (= [[:f 2 3] [:s 3 4] [[:out 2] 2 3]] (released-all (crossing relay)))
        "and releases f, s and the payload f held before")
```

In `collect` below, each pass reads the payload with `fiber/value` and stores
it. Nothing reads it again before the resume at step 6, so the read at step 4
takes it out, and the store moves it into `acc`. The result of each resume is
read again by step 3 or step 4, so it borrows:

```lisp
# (defn collect [g]
#   (def f (fiber/new g |:yield|))   # 0  f is made
#   (def acc @[])                   # 1  acc is made
#   (fiber/resume f)                # 2  f is resumed, and r is its payload
#   (while (not (fiber/done? f))    # 3  f is used, and the test
#     (push acc (fiber/value f))    # 4  v is the payload of f   5  v is stored into acc
#     (fiber/resume f))             # 6  f is resumed, and u is its payload; back to 3
#   acc)                            # 7  acc is used   8  the exit
(def collect (payloads {:next    [[0 1] [1 2] [2 3] [3 4] [4 5] [5 6] [6 3] [3 7] [7 8]]
                        :alloc   [[:f 0] [:acc 1]]
                        :resume  [[:f 2] [:f 6]]
                        :payload [[:r :f 2] [:v :f 4] [:u :f 6]]
                        :store   [[:v :acc 5]]
                        :use     [[:f 2] [:f 3] [:f 4] [:acc 5] [:v 5] [:f 6] [:acc 7]]}))
(assert (= [[:u :f 6] [:r :f 2]] (collect :read)) "each resume's result borrows")
(assert (empty? (refusals collect (ownership collect))) "and v is the fiber's to store")
```

In `twice` below, the payload is read at step 2 and again at step 4, so `a`
borrows. In `cap`, the call of `j` at step 4 may read the
payload through `k`, so `r` borrows. In `pk`, the fiber value is a parameter,
so `r` borrows, and the store of `r` into `acc` is refused. In `ck`, `k` captures
`f` and is never used, so `r` takes the payload out:

```lisp
# (defn twice [g]
#   (def f (fiber/new g |:yield|))   # 0  f is made
#   (fiber/resume f)                # 1  f is resumed, and r is its payload
#   (def a (fiber/value f))         # 2  a is the payload of f
#   (length a)                      # 3  a is used
#   (fiber/value f))                # 4  b is the payload of f   5  the exit
(def twice (payloads {:next    (steps 5)
                      :alloc   [[:f 0]]
                      :resume  [[:f 1]]
                      :payload [[:r :f 1] [:a :f 2] [:b :f 4]]
                      :use     [[:f 1] [:f 2] [:a 3] [:f 4]]}))
(assert (= [[:a :f 2] [:r :f 1]] (twice :read)) "a read before another read borrows")

# (defn cap [g]
#   (def f (fiber/new g |:yield|))   # 0  f is made
#   (def k (fn [] (fiber/value f)))  # 1  k is made, and captures f
#   (def j (fn [] (k)))             # 2  j is made, and captures k
#   (fiber/resume f)                # 3  f is resumed, and r is its payload
#   (j))                            # 4  j is used   5  the exit
(def cap (payloads {:next    (steps 5)
                    :alloc   [[:f 0] [:k 1] [:j 2]]
                    :capture [[:f :k 1] [:k :j 2]]
                    :resume  [[:f 3]]
                    :payload [[:r :f 3]]
                    :use     [[:f 3] [:j 4]]}))
(assert (= [[:r :f 3]] (cap :read)) "a closure that reaches f may read its payload")

# (defn pk [f acc]
#   (push acc (fiber/resume f)))   # 0  f is resumed, and r is its payload
#                                  # 1  r is stored into acc   2  the exit
(def pk (payloads {:next    [[0 1] [1 2]]
                   :param   [[:f 0] [:acc 1]]
                   :resume  [[:f 0]]
                   :payload [[:r :f 0]]
                   :store   [[:r :acc 1]]
                   :use     [[:f 0] [:acc 1] [:r 1]]}))
(assert (= [[:r :acc 1]] (refusals pk (ownership pk))) "a borrowed payload cannot be stored")

# (defn ck [g]
#   (def f (fiber/new g |:yield|))   # 0  f is made
#   (def acc @[])                   # 1  acc is made
#   (def r (fiber/resume f))        # 2  f is resumed, and r is its payload
#   (def k (fn [] (fiber/value f))) # 3  k is made, and captures f
#   (push acc r))                   # 4  r is stored into acc   5  the exit
(def ck (payloads {:next    (steps 5)
                   :alloc   [[:f 0] [:acc 1] [:k 3]]
                   :capture [[:f :k 3]]
                   :resume  [[:f 2]]
                   :payload [[:r :f 2]]
                   :store   [[:r :acc 4]]
                   :use     [[:f 2] [:acc 4] [:r 4]]}))
(assert (empty? (refusals ck (ownership ck))) "the step that makes a closure reads nothing")
```

## Not written yet

- A resume of a fiber value the function does not own takes its old payload
  out, as any removal from an argument does, which
  [signatures](signature.md) does not decide.
