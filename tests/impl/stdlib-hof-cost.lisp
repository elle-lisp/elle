(elle/epoch 15)
# audited: 2026-11-12
# A stdlib search and a stdlib map each cost about what the same loop written in the program costs.
# docs/impl/graft/captures.md
# docs/testing.md
#
# The collection reaches each call through a parameter, so nothing proves it
# an array and no call fuses: the stdlib function runs its own array arm. The
# control is that arm copied into this file, where every wrapper it calls
# grafts to its op.
#
# The counter-factual: with the wrappers inside the stdlib kept as calls, the
# search paid two wrapper calls per element and read about four times the
# control, and the map read about the same ratio.

(def r ((import "std/ratchet")))

(def n 10000)
(def samples 7)
(def gs (->array (string/repeat "x" n)))

(defn my-any [pred coll]
  (letrec [walk (fn (i)
                  (if (>= i (length coll))
                    false
                    (if (pred (get coll i)) true (walk (+ i 1)))))]
    (walk 0)))

(defn my-map [f coll]
  (let* [len (length coll)
         acc @[]]
    (def @i 0)
    (while (< i len)
      (push acc (f (get coll i)))
      (assign i (+ i 1)))
    (freeze acc)))

(defn stdlib-any [coll]
  (any? (fn [c] false) coll))
(defn program-any [coll]
  (my-any (fn [c] false) coll))
(defn stdlib-map [coll]
  (map (fn [c] c) coll))
(defn program-map [coll]
  (my-map (fn [c] c) coll))

(defn search-run [f]
  (fn [] (assert (= (f gs) false) "the search visits every element")))

(defn map-run [f]
  (fn [] (assert (= (length (f gs)) n) "the map answers every element")))

(def subject-any (search-run stdlib-any))
(def control-any (search-run program-any))
(def subject-map (map-run stdlib-map))
(def control-map (map-run program-map))

# Warm up every path so no measurement pays first-call compilation.
(subject-any)
(control-any)
(subject-map)
(control-map)

# The gate compares one loop on two sides, so both sides run on one tier: a
# control compiles exactly when its stdlib function does. A gate across two
# tiers would read the tier, not the graft. The rejections query waits for the
# compiles in flight.
(jit/rejections)
(assert (= (jit? any?) (jit? my-any)) "any? and its control run on one tier")
(assert (= (jit? map) (jit? my-map)) "map and its control run on one tier")

(defn us [seconds]
  (round (* seconds 1000000.0)))

(defn gate [label control subject]
  (println "stdlib-hof-cost: " label " over " n " elements, the fastest of "
           samples ": program " (us control) " us, stdlib " (us subject) " us")
  (assert (< subject (* 2.0 (max control 0.00001)))
          (concat label
                  " in the stdlib must cost about what it costs in the program; its fastest run took "
                  (string (us subject)) " us against the program's "
                  (string (us control)) " us for the same work")))

(def [control-a subject-a] (r:best-of samples control-any subject-any))
(gate "any?" control-a subject-a)

(def [control-m subject-m] (r:best-of samples control-map subject-map))
(gate "map" control-m subject-m)
