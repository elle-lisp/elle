(elle/epoch 13)
# audited: 2026-09-29
# What each colocation pattern costs, in regions retained and pages claimed.
# docs/impl/region/colocation.md
#
# One region owns at least one page, so a structure's footprint follows the
# number of regions its values occupy. Each pattern below is measured in the
# dimension it saves: `arena/region-count` for what a built structure keeps,
# `arena/page-claims` for what a call churns through. Both gauges are
# Immediate, so sampling them allocates nothing.
#
# Three kinds of assertion:
#   - a realized pattern is pinned at its bound, exactly where the bound is
#     exact;
#   - a refusal is pinned from below: the shape the pattern must NOT colocate
#     still keeps one region per value, so a seed that admits churn fails here;
#   - an open pattern is a shrink-only ceiling at today's measure, so the file
#     goes red only when a shape gets worse.

(def window 400)

(defn retained [build]
  "Regions the value BUILD returns still holds once the call is over."
  (def before (arena/region-count))
  (def value (build))
  [(%sub (arena/region-count) before) value])

(defn claims [thunk]
  "Pages claimed per call of THUNK, over WINDOW calls after a warm-up."
  (var i 0)
  (while (%lt i 50)
    (thunk)
    (assign i (%add i 1)))
  (def before (arena/page-claims))
  (var j 0)
  (while (%lt j window)
    (thunk)
    (assign j (%add j 1)))
  (/ (float (%sub (arena/page-claims) before)) window))

(defn exactly [label got want]
  (println "  " label ": " got)
  (assert (= got want)
          (string label ": measured " got ", the pattern's bound is " want)))

(defn at-most [label got ceiling]
  (println "  " label ": " got)
  (assert (<= got ceiling)
          (string label ": measured " got ", over the ceiling of " ceiling)))

(defn at-least [label got floor]
  (println "  " label ": " got)
  (assert (>= got floor)
          (string label ": measured " got ", under the floor of " floor
                  " — the pattern colocated a shape its bound refuses")))

# ── the gauges are live ─────────────────────────────────────────────────────
# Every ceiling below reads green against a dead gauge, so first measure two
# shapes that cannot cost zero. A module-level sink keeps one region per call;
# a cons claims one page per call.

(def sink @[])
(def live-regions
  (get (retained (fn []
                   (var i 0)
                   (while (%lt i 100)
                     (push sink (pair i i))
                     (assign i (%add i 1)))
                   nil)) 0))
(println "region-colocation: gauges")
(at-least "region gauge, 100 conses kept by a module sink" live-regions 100)
(at-least "page gauge, a cons per call" (claims (fn [] (pair 1 2))) 1.0)

# ── Construction: one operation, one region ─────────────────────────────────

(defn collect [& xs]
  xs)
(defn keyed [&keys opts]
  opts)

(println "construction")
(exactly "a rest list of eight arguments, regions"
         (get (retained (fn [] (collect 1 2 3 4 5 6 7 8))) 0) 1)
(at-most "a rest list of four arguments, pages per call"
         (claims (fn [] (collect 1 2 3 4))) 1.0)
(exactly "a &keys struct, regions" (get (retained (fn [] (keyed :a 1 :b 2))) 0)
         1)
(exactly "a native's result, (range 1000), regions"
         (get (retained (fn [] (range 1000))) 0) 1)

# ── Containment: the %pair builder merge ────────────────────────────────────

(println "containment")
(at-most "a local nested %pair, pages per call"
         (claims (fn [] (%first (%pair (%pair 1 2) 3)))) 1.0)
# Open: a returned parent, and a parent built by a constructor native.
(at-most "a returned nested %pair, pages per call (open)"
         (claims (fn [] (%pair (%pair 1 2) 3))) 2.0)
(at-most "a returned [[1 2] [3 4]], regions (open)"
         (get (retained (fn [] [[1 2] [3 4]])) 0) 3)

# ── Cycle: the letrec merge ─────────────────────────────────────────────────

(defn mutual []
  (letrec [ev (fn [n] (if (= n 0) true (od (- n 1))))
           od (fn [n] (if (= n 0) false (ev (- n 1))))]
    ev))

(println "cycle")
(exactly "a returned letrec pair, regions" (get (retained mutual) 0) 1)

# ── Scope arena: a macro expansion ──────────────────────────────────────────
# A bare eval compiles and runs with no transformer. The difference between it
# and an eval whose form expands a macro is what the expansion claims.

(def bare (claims (fn [] (eval '(%add 1 2)))))
(def with-when (claims (fn [] (eval '(when true 1)))))
(def with-case
  (claims (fn []
            (eval '(let [x 1]
                     (case x
                       1 :a
                       2 :b
                       3 :c
                       :d))))))

(println "scope arena")
(at-most "a (when …) expansion, extra pages per eval" (- with-when bare) 2.0)
(at-most "a (case …) expansion, extra pages per eval" (- with-case bare) 2.0)

# ── Append-only container ───────────────────────────────────────────────────

(defn pairs [n]
  (def out @[])
  (var k 0)
  (while (< k n)
    (push out [k k])
    (assign k (+ k 1)))
  out)

(defn strings [n]
  (def out @[])
  (var k 0)
  (while (< k n)
    (push out (string "v" k))
    (assign k (+ k 1)))
  out)

(println "append-only container")
(exactly "1000 pushed pairs, regions" (get (retained (fn [] (pairs 1000))) 0) 1)
(exactly "1000 pushed strings, regions"
         (get (retained (fn [] (strings 1000))) 0) 1)

# The refusals. Each builder below does one thing the bound refuses, so each
# keeps one region per element — the container's own region plus the
# elements it still holds.

(defn pairs-popped [n]
  (def out @[])
  (var k 0)
  (while (< k n)
    (push out [k k])
    (assign k (+ k 1)))
  (pop out)
  out)

(defn measure-of [c]
  (length c))

(defn pairs-passed [n]
  (def out @[])
  (var k 0)
  (while (< k n)
    (push out [k k])
    (assign k (+ k 1)))
  (measure-of out)
  out)

(defn pairs-captured [n]
  (def out @[])
  (def size (fn [] (length out)))
  (var k 0)
  (while (< k n)
    (push out [k k])
    (assign k (+ k 1)))
  (size)
  out)

(defn pairs-held [n]
  (def out @[])
  (var k 0)
  (var last nil)
  (while (< k n)
    (def p [k k])
    (push out p)
    (assign last p)
    (assign k (+ k 1)))
  [out last])

(at-least "a builder that pops, regions"
          (get (retained (fn [] (pairs-popped 1000))) 0) 1000)
(at-least "a builder that passes its container to a function, regions"
          (get (retained (fn [] (pairs-passed 1000))) 0) 1001)
(at-least "a builder whose container a closure captures, regions"
          (get (retained (fn [] (pairs-captured 1000))) 0) 1001)
(at-least "a builder that keeps each pushed value in a binding, regions"
          (get (retained (fn [] (pairs-held 1000))) 0) 1001)

# ── Accumulator (open) ──────────────────────────────────────────────────────

(defn conses [n]
  (var acc ())
  (var k 0)
  (while (< k n)
    (assign acc (pair k acc))
    (assign k (+ k 1)))
  acc)

(println "accumulator")
(at-most "1000 consed onto an accumulator, regions (open)"
         (get (retained (fn [] (conses 1000))) 0) 1000)

(println "region-colocation: ok")
