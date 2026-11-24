(elle/epoch 18)
# audited: 2026-11-23
# A compiled self-recursion costs a bounded multiple of the self tail loop that makes the same number of trips.
# docs/impl/jit-selfcall.md
# docs/testing.md
#
# `count-down` calls itself 2000 deep with an integer argument, so each level
# is a direct self-call. `loop-down` makes the same 2000 trips as a self tail
# call, which compiles to a jump. Both are timed on the thread's CPU clock.
#
# The counter-factual: every self-call through `elle_jit_call`, which resolves
# the callee, looks up its code and hands each parameter a reference. The
# recursion then read about 18.5 times the loop on the machine this bound was
# set on.

(def r ((import "std/ratchet")))

(def depth 2000)
(def rounds 50)
(def samples 9)

(defn count-down [n]
  (if (= n 0)
    0
    (+ 1 (count-down (- n 1)))))

(defn loop-down [n acc]
  (if (= n 0) acc (loop-down (- n 1) (+ acc 1))))

(assert (= (count-down depth) depth) "the recursion counts every level")
(assert (= (loop-down depth 0) depth) "the loop counts every trip")

(defn nested []
  (repeat rounds (count-down depth)))
(defn looped []
  (repeat rounds (loop-down depth 0)))

# Warm up both, so neither measurement pays a first compile.
(nested)
(looped)

(def [control subject] (r:best-of samples looped nested))

(defn us [seconds]
  (round (* seconds 1000000.0)))

(println "jit-selfcall-cost: the fastest of " samples ": loop " (us control)
         " us, recursion " (us subject) " us")

(assert (< subject (* 14.0 (max control 0.00001)))
        (concat "a direct self-call must cost a bounded multiple of a loop trip; "
                "the recursion took " (string (us subject))
                " us against the loop's " (string (us control)) " us"))
