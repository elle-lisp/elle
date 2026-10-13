(elle/epoch 15)
# audited: 2026-10-12
# An expansion runs every call on the interpreter, so silent compiled code cannot outrun its budget.
# docs/runtime.md
#
# The sidecar makes the JIT eager and compiles on the VM thread, so a function
# runs as compiled code from its second call. Each `eval` below expands on this
# VM, whose JIT the sidecar configured. `helper` is silent: its loop holds no
# call, so compiled, it spends the fuel word and never tests the budget.
#
# The counter-factual: an expansion that reaches the JIT compiles `helper` on
# its first call, so the probe answers true, and `spin` runs the compiled loop,
# which no budget can halt, until the rig's deadline.

## No call during an expansion is profiled, so nothing compiles.
(def probed
  (eval '(begin
           (begin-for-syntax (def helper
                               (fn [go]
                                 (while go nil)
                                 0)))
           (defmacro probe []
             (begin
               (helper false)
               (helper false)
               (jit? helper)))
           (probe))))
(assert (= false probed) "the helper stays interpreted during the expansion")

## A silent loop after the warm-up spends the expansion budget, and halts.
(def [spun? spin-err]
  (protect (eval '(begin
                    (begin-for-syntax (def helper
                                        (fn [go]
                                          (while go nil)
                                          0)))
                    (defmacro spin []
                      (begin
                        (helper false)
                        (helper false)
                        (helper true)))
                    (spin)))))
(assert (not spun?) "the expansion fails")
(assert (string/contains? (get spin-err :message) "macro 'spin'")
        "naming the macro")
(assert (string/contains? (get spin-err :message) "fuel") "and its budget")

## compile/run-on refuses a compiled tier during an expansion, and the bytecode
## tier still runs.
(def [forced? forced-err]
  (protect (eval '(begin
                    (defmacro forced []
                      (compile/run-on :jit (fn [] 1)))
                    (forced)))))
(assert (not forced?) "the compiled tier is refused")
(assert (string/contains? (get forced-err :message) ":tier-rejected")
        "with the tier's refusal")
(assert (string/contains? (get forced-err :message)
                          "compile/run-on :jit cannot run during a macro expansion")
        "naming the expansion")
(assert (= 7
           (eval '(begin
                    (defmacro interpreted []
                      (compile/run-on :bytecode (fn [] 7)))
                    (interpreted))))
        "the bytecode tier runs during an expansion")

(println "expansion-fuel-jit: ok")
