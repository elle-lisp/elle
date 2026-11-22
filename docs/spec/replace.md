# Resume replaces, in Datalog

<!-- audited: 2026-11-22 -->

How a resume ends the fiber value's hold on each value it held, so the new payload waits for none of them, as Datalog on proof.

[Payload](payload.md) reads what a resume leaves.
[The ownership rules](ownership.md) state the Resume replaces rule in words, and
[removal](removal.md) holds the waits a removal adds.

## The rules

| Rule | Datalog |
|------|---------|
| Resume replaces | `moved(e, f, p) :- resume(f, p), read(e, f, p0).` |
| Resume replaces | `alloc(v, p) :- takes(v, f, p), resume(f, p).` |
| Resume replaces | `remove(v, f, p) :- takes(v, f, p), not resume(f, p).` in place of the Payload clause |

A fiber value holds payloads alone, and each value read out of it between two
resumes is the payload the first of them left. The second resume takes that
payload out as `[:out p]`, so the fiber value holds none of those reads after
it, and the Removal rule still lets `[:out p]` be any of them. A read that
takes the new payload out at the resume is new to this function, as the result
of an emit is, so the step makes it.

The `moved` clause reads facts alone, so it passes to `ownership` as `:moved`.
A read that the fiber value does not hold at the resume has no hold to end.

In `rp`, `fiber/status` uses `f` at step 2, so `r` borrows. The resume at step
3 takes `r` out as `[:out 3]`, which is released after the use of `r` at step
4. The read of `s` takes the new payload out at the same step, so `s` is
released at once, and `f` no longer holds `r`, so `f` is released after its
last use. Under the Payload rule alone, `s` and `f` wait for the use of `r`.

In `rv`, `s` borrows, because `fiber/value` reads the payload again at step 4,
and that read takes it out as `v`. The resume at step 3 ends the hold of `f` on
`r`, so `v` may be `s` and not `r`, and `f` holds no value that is used after
step 4. Only `[:out 3]` waits for the use of `r` at step 6:

```lisp
(fn []
  (def proof ((import "std/proof")))
  (def {:fence fence} ((import "../spec.md")))
  (def {:steps steps} ((import "./datalog.md")))
  (def {:released-all released-all} ((import "./capture.md")))
  (def {:payloads payloads} ((import "./payload.md")))

  (defn replaced [facts]
    "FACTS after payloads, with each take at a resume made, and each read out of a fiber value
     moved out of it at each resume, under the Resume replaces rule."
    (def out (payloads facts))
    (def resume (proof:relation (get facts :resume [])))   # [f p]
    # takes(v, f, p) :- payload(v, f, p), not read(v, f, p).
    (def takes (proof:antijoin (proof:relation (map (fn [t] [t true]) (get facts :payload [])))
                               (proof:relation (out :read)) (fn [t _] t)))
    # alloc(v, p) :- takes(v, f, p), resume(f, p).
    (def made (proof:join (proof:relation (map (fn [[v f p]] [[f p] v]) (proof:elements takes)))
                          (proof:map resume (fn [fp] [fp true])) (fn [[f p] v _] [v f p])))
    # remove(v, f, p) :- takes(v, f, p), not resume(f, p).
    (def removed (proof:antijoin (proof:relation (map (fn [t] [t true]) (out :remove)))
                                 made (fn [t _] t)))
    (merge out {:alloc  (concat (get out :alloc []) (map (fn [[v f p]] [v p]) (proof:elements made)))
                :remove (proof:elements removed)
                # moved(e, f, p) :- resume(f, p), read(e, f, p0).
                :moved  (proof:elements (proof:join (proof:relation (map (fn [[e f p0]] [f e]) (out :read)))
                                                    resume (fn [f e p] [e f p])))}))

  (def rp (fence 'rp
    :doc "The new payload and the fiber value wait for none of the values the resume took out."
    :source "(defn rp [g]
  (def f (fiber/new g |:yield|))   # 0  f is made
  (def r (fiber/resume f))        # 1  f is resumed, and r is its payload
  (fiber/status f)                # 2  f is used
  (fiber/resume f)                # 3  f is resumed, and s is its payload
  (length r))                     # 4  r is used   5  the exit"
    :facts {:next    (steps 5)
            :alloc   [[:f 0]]
            :resume  [[:f 1] [:f 3]]
            :payload [[:r :f 1] [:s :f 3]]
            :use     [[:f 1] [:f 2] [:f 3] [:r 4]]}
    :check (comp (fn [rp] (= [[:f 3 4] [:s 3 4] [[:out 1] 1 2] [[:out 3] 4 5]] (released-all rp)))
                 replaced)))

  (def rv (fence 'rv
    :doc "v takes the payload out, and waits for none of the values an earlier resume took out."
    :source "(defn rv [g]
  (def f (fiber/new g |:yield|))   # 0  f is made
  (def r (fiber/resume f))        # 1  f is resumed, and r is its payload
  (fiber/status f)                # 2  f is used
  (fiber/resume f)                # 3  f is resumed, and s is its payload
  (def v (fiber/value f))         # 4  v is the payload of f
  (length v)                      # 5  v is used
  (length r))                     # 6  r is used   7  the exit"
    :facts {:next    (steps 7)
            :alloc   [[:f 0]]
            :resume  [[:f 1] [:f 3]]
            :payload [[:r :f 1] [:s :f 3] [:v :f 4]]
            :use     [[:f 1] [:f 2] [:f 3] [:f 4] [:v 5] [:r 6]]}
    :check (comp (fn [rv] (and (= [[:v :f 4] [[:out 1] :f 1] [[:out 3] :f 3]] (rv :remove))
                               (= [[:f 4 5] [:v 5 6] [[:out 1] 1 2] [[:out 3] 6 7]] (released-all rv))))
                 replaced)))

  {:replaced replaced :fences [rp rv]})
```
