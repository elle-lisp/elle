(elle/epoch 12)
# Regression: a closure whose LIR embeds a COMPOUND constant (a quoted list,
# struct, …) must keep its LIR across an os/spawn boundary, so the optimizing
# tiers can still run it in the worker thread.
#
# Before the fix, os/spawn's serializer (src/value/send.rs) dropped the whole
# LIR whenever convert_value_consts_for_send hit an unsendable compound
# ValueConst — e.g. the (quote (= …)) the `assert` macro bakes into its failure
# payload. compile/run-on :jit then reported "closure has no LIR" (:ineligible).
# The fix lifts such operands into a sendable `lir_value_pool` (LirConst::ValueRef)
# and patches them back to ValueConst on receipt. See docs/test-runner.md
# § Tiers and the agent-first runner's cross-tier execution.

# Probe whether the :jit tier is compiled into this build (force-compile a
# trivial closure in a worker; :feature-disabled means no JIT here).
(defn jit-available? []
  (let [r (os/join (os/spawn-vm (fn [] (protect (compile/run-on :jit (fn [] 0))))))]
    (not (and (not (get r 0)) (= (get (get r 1) :reason) :feature-disabled)))))

# Compile in the main thread (has the symbol table), then ship the closure to a
# worker and force it onto :jit there — exactly the test runner's mechanism.
(defn run-on-jit-in-worker [form]
  (let [thunk (eval (list (quote fn) [] form))]
    (os/join (os/spawn-vm (fn [] (protect (compile/run-on :jit thunk)))))))

# Ship an ALREADY-BUILT closure (one that captures upvalues) to a worker and run
# it on :jit there. The fault-barrier compile mode hands the runner thunks that
# capture the file's shared bindings, so the send must preserve captured upvalues
# (including captured closures) alongside the quoted-compound LIR.
(defn ship-to-jit [thunk]
  (os/join (os/spawn-vm (fn [] (protect (compile/run-on :jit thunk))))))

(when (jit-available?)  # The assert macro embeds (quote (= (+ 1 1) 2)) — a compound — in its payload.
  # Its LIR must survive the send so :jit can run it.
  (let [r (run-on-jit-in-worker (quote (assert (= (+ 1 1) 2) "lir survives send")))]
    (assert (get r 0)
            (string "assert closure must run on :jit after send (LIR kept), got "
                    r))
    (assert (= (get r 1) true) "the passing assert returns true on :jit"))

  # A closure returning a bare quoted list: the list is a compound ValueConst;
  # it must round-trip by value through the pool and come back intact.
  (let [r (run-on-jit-in-worker (quote (quote (a b c))))]
    (assert (get r 0)
            (string "quoted-list closure must run on :jit after send, got " r))
    (assert (= (length (get r 1)) 3)
            "the quoted list survives with all 3 elements"))

  # Sanity: the same path with no compound constant already worked — guard it too.
  (let [r (run-on-jit-in-worker (quote (+ 40 2)))]
    (assert (get r 0) "scalar closure runs on :jit after send")
    (assert (= (get r 1) 42) "scalar closure returns 42 on :jit"))

  # Barrier-mode shape: a thunk capturing an upvalue (a value AND a captured
  # helper closure) AND embedding the assert macro's quoted compound. This is
  # exactly what `compile/barrier-module` produces for a test form that uses an
  # earlier `def`/`defn`. It must survive the send and run on :jit.
  (let [base 41
        helper (fn [x] (+ x base))
        thunk (fn [] (assert (= (helper 1) 42) "captured upvalues survive send"))
        r (ship-to-jit thunk)]
    (assert (get r 0)
            (string "upvalue-capturing thunk must run on :jit after send, got "
                    r))
    (assert (= (get r 1) true)
            "the passing assert over captured upvalues returns true on :jit")))

(println "send-lir cross-thread JIT tests passed")
