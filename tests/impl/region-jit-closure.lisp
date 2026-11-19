(elle/epoch 15)
# audited: 2026-11-18
# A closure compiled code builds is reclaimed as the interpreter reclaims it, and a returned one stays live through churn.
# docs/impl/jit-closure.md
# docs/impl/region/letrec.md
#
# Compiled code builds a closure with the function the interpreter's
# `MakeClosure` handler calls, in the region the instruction names, and a
# merged slot resolves through the mint-or-reuse helper. Four shapes read
# that:
#
# - `adders` builds a closure on each trip of a loop. Each one is released on
#   its trip, so a call leaves nothing behind.
# - `parity` builds a mutually recursive pair, whose cycle merges into one
#   arena. The arena is freed once per call.
# - `make-self` returns the self-recursive closure it builds, and `counter`
#   returns a closure over a celled local. Each handle is re-entered after the
#   pages a premature release would free have been allocated over.
#
# A release that never runs strands objects per call, which the gauge reads. A
# release that runs twice, or before the last read, faults under
# region-jit-closure-guardfree.lisp. A JIT that refuses a function that builds
# a closure runs every shape interpreted, and only `jit?` tells.
#
# The sidecar makes the JIT eager on the VM thread and turns the graft off.

(defn adders [n]
  (var s 0)
  (var i 0)
  (while (< i n)
    (let [f (fn [x] (+ x i))]
      (assign s (+ s (f 1))))
    (assign i (+ i 1)))
  s)

(defn parity [n]
  (letrec [ev (fn [m] (if (= m 0) true (od (- m 1))))
           od (fn [m] (if (= m 0) false (ev (- m 1))))]
    (ev n)))

(defn make-self [n]
  (defn go [m]
    (when (%not (%int? m)) (error :m))
    (if (%lt m 1) go (go (%sub m 1))))
  (go n))

(defn counter [start]
  (var c start)
  (fn []
    (assign c (+ c 1))
    c))

# ── reclaimed per call ──────────────────────────────────────────────────────
(def window 200)

(defn measure [thunk]
  (repeat 30 (thunk))
  (jit/rejections)
  (def before (arena/count))
  (repeat window (thunk))
  (%sub (arena/count) before))

(defn bounded? [d label]
  (assert (%lt d 60) (string label " leaks, delta=" d)))

(bounded? (measure (fn [] (assert (= (adders 10) 55) "adders sums")))
          "a closure per trip")
(bounded? (measure (fn [] (assert (parity 8) "8 is even")))
          "a merged letrec cycle")
(bounded? (measure (fn [] (make-self 3))) "a returned self-recursive closure")
(bounded? (measure (fn [] ((counter 0)))) "a returned closure over a cell")

(when (vm/config :jit)
  (assert (and (jit? adders) (jit? parity) (jit? make-self) (jit? counter))
          "the functions that build the closures compile"))

# ── returned handles stay live through churn ────────────────────────────────
(def parked @[])
(def churn @[])
(var i 0)
(while (< i 60)
  (let [h (make-self 3)
        c (counter i)]
    (push parked [h c i])
    (push churn (string "churn-" i))
    (push churn @[i i i])
    (assert (fn? (h 1)) "a self-recursive handle stale after churn")
    (assert (= (c) (+ i 1)) "a counter handle stale after churn"))
  (assign i (+ i 1)))

(each [h c k] parked
  (assert (fn? (h 2)) "a parked self-recursive handle re-enters")
  (assert (= (c) (+ k 2)) "a parked counter reads and writes its own cell"))

(println "region-jit-closure: ok")
