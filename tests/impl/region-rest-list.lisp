(elle/epoch 14)
# audited: 2026-10-06
# A rest list no cell of which can outlive its head takes one region; one that can keeps a region per cell.
# docs/impl/region/restlist.md
#
# The admitted half reads page claims. A callee that only reads its rest list
# claims the same pages for six arguments as for one. Counterfactual: a list
# built one region per cell claims a page per argument, so six arguments cost
# five pages more than one.
#
# The refused half reads the live object count. When a callee keeps a tail of
# its rest list, the head cell is freed when the call ends, so the caller holds
# one object fewer than when the callee keeps the whole list. Counterfactual: a
# gate that admits the shape builds the head into the tail's region, and the
# two counts read the same.
#
# Each half runs on the interpreter, before the JIT threshold, and again once
# the function is compiled. The sidecar compiles on the VM thread, so `jit?`
# answers as soon as the threshold is crossed. A build without the JIT runs the
# interpreter half alone.

(def jit-build? (not (nil? (vm/config :jit))))

(defn pages-per-call [thunk]
  "Page claims of one call of THUNK."
  (let [p0 (arena/page-claims)]
    (thunk)
    (%sub (arena/page-claims) p0)))

(defn live-gain [thunk]
  "Objects still live once THUNK's call has returned, its value held."
  (let [o0 (arena/count)
        v (thunk)
        o1 (arena/count)]
    (assert (not (nil? v)) "the value is held across the reading")
    (%sub o1 o0)))

# The trap: a tail call does not advance its callee's JIT count, so a thunk
# whose body tail-calls the function compiles itself and never the function.
# Every thunk below hands its call's value to `held`, which keeps the call out
# of tail position.
(defn held [v]
  "V, from a fixed-arity call that claims no page."
  v)

(defn compile-past-threshold [f thunk]
  "Call THUNK past the JIT threshold, and check that F was compiled."
  (repeat 20 (thunk))
  (when jit-build? (assert (jit? f) "the function compiled past the threshold")))

# ── The admitted shapes: the list is only read ───────────────────────

(defn count-args [& xs]
  "Length, an Immediate primitive."
  (length xs))

(defn none? [& xs]
  "empty?, an Immediate primitive."
  (empty? xs))

(defn same? [& xs]
  "=, an Immediate primitive, given the list twice."
  (= xs xs))

(defn first-of [& xs]
  "first, which answers an element."
  (first xs))

(defn second-of [& xs]
  "second, which answers an element."
  (second xs))

(defn kind-of [& xs]
  "%type-of, an intrinsic that needs no proof."
  (%type-of xs))

(defn spliced [& xs]
  "A spliced call: the callee receives the elements."
  (string ;xs))

(defn applied [& xs]
  "apply, which expands to a spliced call."
  (apply string xs))

(defn read-twice [& xs]
  "Two admitted reads on two paths."
  (if (empty? xs) 0 (first xs)))

(defn same-pages [label f one six]
  "F's call with one argument claims what its call with six does."
  (let [p1 (pages-per-call one)
        p6 (pages-per-call six)]
    (assert (= p1 p6)
            (string label ": one argument claimed " p1 " pages, six claimed " p6))))

(defn admitted [label f one six]
  "Warm both calls, read them on the interpreter, then compiled."
  (one)
  (one)
  (six)
  (six)
  (same-pages (string label ", interpreted") f one six)
  (compile-past-threshold f six)
  (same-pages (string label ", compiled") f one six))

(admitted "length" count-args (fn [] (held (count-args 1)))
          (fn [] (held (count-args 1 2 3 4 5 6))))
(admitted "empty?" none? (fn [] (held (none? 1)))
          (fn [] (held (none? 1 2 3 4 5 6))))
(admitted "=" same? (fn [] (held (same? 1))) (fn [] (held (same? 1 2 3 4 5 6))))
(admitted "first" first-of (fn [] (held (first-of 1)))
          (fn [] (held (first-of 1 2 3 4 5 6))))
(admitted "second" second-of (fn [] (held (second-of 1 2)))
          (fn [] (held (second-of 1 2 3 4 5 6))))
(admitted "%type-of" kind-of (fn [] (held (kind-of 1)))
          (fn [] (held (kind-of 1 2 3 4 5 6))))
(admitted "splice" spliced (fn [] (held (spliced 1)))
          (fn [] (held (spliced 1 2 3 4 5 6))))
(admitted "apply" applied (fn [] (held (applied 1)))
          (fn [] (held (applied 1 2 3 4 5 6))))
(admitted "two reads" read-twice (fn [] (held (read-twice 1)))
          (fn [] (held (read-twice 1 2 3 4 5 6))))

# An admitted list still releases what its cells hold. Each element here is a
# heap value in a region of its own, which the list's cells count, so a list
# region that died without releasing them would strand three per call.
(def slack 16)

(defn drift [thunk]
  "Live objects gained across THUNK."
  (let [o0 (arena/count)]
    (thunk)
    (%sub (arena/count) o0)))

(let [d (drift (fn []
                 (let [@i 0]
                   (while (%lt i 2000)
                     (count-args (%pair i i) (%pair i i) (%pair i i))
                     (first-of (%pair i i) (%pair i i) (%pair i i))
                     (assign i (%add i 1))))))]
  (assert (%lt d slack)
          (string "admitted lists with heap elements gained " d " live objects")))

# ── The refused shapes: a tail can outlive the head ──────────────────

(defn keeps [& xs]
  "Returns the whole list: the control every refused shape is read against."
  xs)

(defn tail-of [& xs]
  "rest, which answers a tail."
  (rest xs))

(defn my-rest [l]
  "A closure that takes a tail."
  (rest l))

(defn via-closure [& xs]
  "The list passed unspliced to a closure."
  (my-rest xs))

(defn via-alias [& xs]
  "The list bound to another name."
  (let [ys xs]
    (rest ys)))

(defn via-capture [& xs]
  "The list captured by a nested lambda."
  ((fn [] (rest xs))))

(def sink @[])

(defn stores-tail [& xs]
  "A tail stored in a container that outlives the call."
  (push sink (rest xs))
  sink)

(defn stores-whole [& xs]
  "The whole list stored: the control for stores-tail."
  (push sink xs)
  sink)

(defn head-freed [label tail-thunk whole-thunk]
  "The kept tail holds one object fewer than the kept whole list."
  (let [t (live-gain tail-thunk)
        w (live-gain whole-thunk)]
    (assert (= t (%sub w 1))
            (string label ": the tail kept " t " live objects, the whole list "
                    w))))

(defn refused-interpreted [label tail-thunk whole-thunk]
  "Read the shape on the interpreter."
  (tail-thunk)
  (whole-thunk)
  (head-freed (string label ", interpreted") tail-thunk whole-thunk))

(defn refused [label f tail-thunk whole-thunk]
  "Read the shape on the interpreter, then compiled."
  (refused-interpreted label tail-thunk whole-thunk)
  (compile-past-threshold f tail-thunk)
  (head-freed (string label ", compiled") tail-thunk whole-thunk))

(def whole (fn [] (held (keeps 1 2 3 4))))

(refused "rest" tail-of (fn [] (held (tail-of 1 2 3 4))) whole)
(refused "a closure" via-closure (fn [] (held (via-closure 1 2 3 4))) whole)
(refused "an alias" via-alias (fn [] (held (via-alias 1 2 3 4))) whole)
# The JIT refuses a function that builds a closure, so this shape runs on the
# interpreter alone.
(refused-interpreted "a capture" (fn [] (held (via-capture 1 2 3 4))) whole)
(refused "a store" stores-tail (fn [] (held (stores-tail 1 2 3 4)))
         (fn [] (held (stores-whole 1 2 3 4))))

(println "region-rest-list: ok")
