#!/usr/bin/env elle
(elle/epoch 12)

# tests/elle/jit-double-import-uaf.lisp
#
# Family D regression — heap corruption under --jit=eager.
#
# Recipe:
#  1. import a module file once at top level (lib/http.lisp)
#  2. import a second module whose body re-imports the first via
#     (import "std/http"), creating a second instance of the same source
#  3. spawn a server fiber that calls into one of the http instances
#  4. run a yield-loop on main (ev/sleep)
#
# Under --jit=eager the JIT compiles every closure (including the two
# module bodies that resolve to the same source file). The eight
# sleep-loop iterations corrupt the C heap reliably ("malloc():
# unsorted double linked list corrupted"). The same script under
# --jit=off and under default --jit=adaptive runs cleanly.
#
# Pair this with --jit=off to confirm the JIT-specific causality:
#   smoke-vm passes; smoke-jit fails.

(def http ((import-file "lib/http.lisp")))
(def telemetry ((import-file "lib/telemetry.lisp")))

(def received @[])
(defn collector-handler [request]
  (push received request:body)
  (http:respond 200 "ok"))

(def listener (tcp/listen "127.0.0.1" 0))
(def server (ev/spawn (fn [] (http:serve listener collector-handler))))

(defn timed [thunk]
  (let* [start (clock/monotonic)
         result (thunk)
         elapsed (- (clock/monotonic) start)]
    elapsed))

(println "  simulating...")
(def @i 0)
(while (< i 8)
  (timed (fn [] (ev/sleep 0.001)))
  (assign i (+ i 1)))
(println "  simulated")

(ev/abort server)
(port/close listener)
(println "")
(println "all jit-double-import-uaf tests passed.")
