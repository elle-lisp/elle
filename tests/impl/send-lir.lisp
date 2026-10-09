(elle/epoch 14)
# audited: 2026-10-06
# A closure shipped to a worker keeps its LIR, so the worker's JIT compiles it.
# docs/threads.md
#
# A worker rebuilds a closure from its bundle, and the LIR has to arrive with
# it. A sender that dropped the LIR would leave the closure running correctly on
# the interpreter, so each case below forces the worker onto :jit, where a
# closure with no LIR is rejected (:ineligible) rather than answered.
#
# The cases cover what the LIR carries: a quoted compound, a call into the
# standard library (a stdlib function is a closure the LIR loads as a
# `ValueConst`), a scalar, and a thunk over captured upvalues.

# Probe whether the :jit tier is compiled into this build (force-compile a
# trivial closure in a worker). A build with no JIT rejects with :error
# :tier-rejected — keyed on :error, not :reason, since the rejection carries
# :ineligible (the run_on stub) OR :feature-disabled (the tier signal) depending
# on the path; both share :tier-rejected.
(defn jit-available? []
  (let [r (os/join (os/spawn-vm (fn [] (protect (compile/run-on :jit (fn [] 0))))))]
    (not (and (not (get r 0)) (= (get (get r 1) :error) :tier-rejected)))))

# Compile in the main thread (has the symbol table), then ship the closure to a
# worker and force it onto :jit there — exactly the test runner's mechanism.
(defn run-on-jit-in-worker [form]
  (let [thunk (eval (list (quote fn) [] form))]
    (os/join (os/spawn-vm (fn [] (protect (compile/run-on :jit thunk)))))))

# Ship an ALREADY-BUILT closure (one that captures upvalues) to a worker and run
# it on :jit there. The fault-barrier compile mode hands the runner thunks that
# capture the file's shared bindings, so the send must preserve captured upvalues
# (including captured closures) alongside the LIR.
(defn ship-to-jit [thunk]
  (os/join (os/spawn-vm (fn [] (protect (compile/run-on :jit thunk))))))

(when (jit-available?)
  # The assert macro embeds (quote (= (+ 1 1) 2)) — a compound — in its payload.
  (let [r (run-on-jit-in-worker (quote (assert (= (+ 1 1) 2) "lir survives send")))]
    (assert (get r 0)
            (string "assert closure must run on :jit after send (LIR kept), got "
                    r))
    (assert (= (get r 1) true) "the passing assert returns true on :jit"))

  # A closure returning a bare quoted list: the list must come back intact.
  (let [r (run-on-jit-in-worker (quote (quote (a b c))))]
    (assert (get r 0)
            (string "quoted-list closure must run on :jit after send, got " r))
    (assert (= (length (get r 1)) 3)
            "the quoted list survives with all 3 elements"))

  # A call into the standard library: `inc` is a stdlib closure, which the LIR
  # loads as a `ValueConst`. The light worker has no stdlib of its own, so the
  # closure it calls is the one that crossed in the bundle.
  (let [r (ship-to-jit (fn [] (inc 41)))]
    (assert (get r 0)
            (string "a closure calling stdlib must run on :jit after send, got "
                    r))
    (assert (= (get r 1) 42) "the stdlib call answers on :jit"))

  # The same path with no compound constant.
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
