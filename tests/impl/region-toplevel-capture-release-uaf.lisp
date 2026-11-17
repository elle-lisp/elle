(elle/epoch 16)
# audited: 2026-11-17
# A top-level def released at its last use stays readable through every closure and container that still holds it.
# docs/impl/region/cells.md
# docs/impl/region/lastuse.md
#
# The soundness complement of region-toplevel-capture-release.lisp. The
# sidecar arms guardfree, so a released page is unmapped and a stale read
# faults at its deref. Each row lets the binding's last direct read pass,
# churns allocations, and then reads the value through what still holds it.
#
# The counter-factual: a release that followed the binding's direct reads and
# missed the hold a closure or a container takes would free the value under
# that holder.

(defn churn []
  (var i 0)
  (while (< i 300)
    (array i (string i) @[i])
    (assign i (+ i 1))))

# (a) a closure stored in a container outlives the def's last direct read.
(def a-v @[11 12])
(def a-box @[])
(push a-box (fn [] (first a-v)))
(churn)
(def a-read ((get a-box 0)))

# (b) a closure handed to a fiber, resumed after the churn.
(def b-v @[21 22])
(def b-fiber
  (fiber/new (fn []
               (yield (first b-v))
               (first b-v)) |:yield|))
(churn)
(def b-read (fiber/resume b-fiber))

# (c) a destructured def, captured.
(def [c-a c-b] [@[51] @[52]])
(defn c-read-fn []
  (first c-b))
(churn)
(def c-read (c-read-fn))

# (d) a reassigned captured def, read before and after the reassignment.
(def @d-v @[61])
(defn d-read-fn []
  (first d-v))
(def d-first (d-read-fn))
(assign d-v @[62])
(churn)
(def d-read (d-read-fn))

# (e) a def in a top-level begin, through a closure the begin defines.
(begin
  (def e-v @[71])
  (defn e-read-fn []
    (first e-v)))
(churn)
(def e-read (e-read-fn))

# (f) a letrec in a function returns the closure that captures its sibling.
(defn f-make []
  (letrec [v @[81]
           g (fn [] (first v))]
    g))
(def f-g (f-make))
(churn)
(def f-read (f-g))

# (g) a let in a function stores the closure that captures its sibling.
(def g-sink @[])
(defn g-make []
  (let [v @[91]
        g (fn [] (first v))]
    (push g-sink g)
    0))
(g-make)
(churn)
(def g-read ((get g-sink 0)))

(assert (= a-read 11) "a closure in a container reads the captured def")
(assert (= b-read 21) "a fiber reads the captured def")
(assert (= c-read 52) "a destructured def is read through a closure")
(assert (= d-first 61) "a reassigned def reads its first value")
(assert (= d-read 62) "a reassigned def reads its second value")
(assert (= e-read 71) "a def in a begin is read through its closure")
(assert (= f-read 81) "a letrec capture outlives its function")
(assert (= g-read 91) "a let capture outlives its function")

(println "region-toplevel-capture-release-uaf: ok")
