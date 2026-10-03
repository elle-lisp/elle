(elle/epoch 12)
# audited: 2026-09-29
# A capability denied in tail position pauses the fiber with a readable denial payload, interpreted and compiled.
# docs/impl/region/park.md
#
# region-capability-denial-value.lisp covers Call position. Here the denied
# `:io` primitive sits in TAIL position of a named function that the fiber
# tail-calls, so the denial is decided on a tail-call path. The interpreter
# denies in `tail_call_inner` (src/vm/call/inner/tail.rs), and the JIT's native
# dispatch (`elle_jit_call`, `elle_jit_tail_call`, src/jit/calls/) must deny
# the same way.
#
# The counter-factual: a native dispatch that skipped the
# `def.signal ∩ withheld ∩ CAP_MASK` gate runs the withheld primitive and
# suspends on its raw effect request, so `fiber/value` reads an `io-request`
# instead of the `:capability-denied` payload. The loop's first iterations run
# interpreted, and the later ones run the body after the JIT compiles it.

# port/write in genuine tail position of a named fn (JIT-compiled once hot).
(defn write-blocked []
  (port/write (*stdout*) "should be blocked"))

(defn tail-denied []
  (let [f (fiber/new (fn [] (write-blocked)) |:error :io| :deny |:io|)]
    (fiber/resume f)
    (assert (= (fiber/status f) :paused) "fiber pauses after tail :io denial")
    (let [val (fiber/value f)]
      (assert (= :capability-denied (get val :error))
              "tail-position denial payload :error survives resume")
      (assert (= "port/write" (get val :primitive))
              "tail-position denial names the blocked primitive")
      val)))

# Loop with intervening heap churn (like the Call-position fixture) so a
# prematurely-freed payload region would be recycled and fault a later read, and
# so the fiber body warms up to the JIT.
(each i (range 0 400)
  (let [v (tail-denied)]
    (def junk (@string))
    (%string-push junk "yyyyyyyyyyyyyyyyyyyyyyyyyyyyyyyy")
    (assert (= :capability-denied (get v :error))
            "tail denial payload still valid after intervening allocation")))

(println "region-capability-denial-tail: OK")
