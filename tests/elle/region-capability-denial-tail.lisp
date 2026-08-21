(elle/epoch 12)
# tests/elle/region-capability-denial-tail.lisp
#
# Companion to region-capability-denial-value.lisp (Call position). Here the
# denied `:io` primitive sits in TAIL position of a named function that the fiber
# tail-calls, so denial is decided on a tail-call path. The capability gate must
# be tier-independent: the interpreter denies in `tail_call_inner`
# (src/vm/call/inner/tail.rs) and the JIT must deny identically in its native
# dispatch paths (`elle_jit_call` / `elle_jit_tail_call`, src/jit/calls/) — a JIT
# native dispatch that skipped the `def.signal ∩ withheld ∩ CAP_MASK` gate would
# run the withheld primitive and suspend on its raw effect request, so
# `fiber/value` would read an `io-request` instead of the `:capability-denied`
# payload. Run under every tier (the corpus runs vm + jit); the loop makes the
# body hot so the JIT compiles it.

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
