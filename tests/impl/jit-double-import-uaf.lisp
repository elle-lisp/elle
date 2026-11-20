#!/usr/bin/env elle
(elle/epoch 14)
# audited: 2026-10-06
# Two instances of one module, every function compiled eagerly, run a server fiber and a sleep loop without corrupting the heap.
# docs/impl/jit.md
#
# The sidecar compiles every function on its first call, the mode that
# corrupted the C heap. The default policy and the interpreter run it cleanly.
#
# Recipe:
#  1. import a module file once at top level (lib/http.lisp)
#  2. import a second module whose body re-imports the first via
#     (import "std/http"), creating a second instance of the same source
#  3. spawn a server fiber that calls into one of the http instances
#  4. run a yield-loop on main (ev/sleep)
#
# Compiled eagerly, the JIT compiles every closure, the two module bodies that
# resolve to the same source file included. The counter-factual is the C heap
# corrupting within the eight sleep-loop iterations ("malloc(): unsorted double
# linked list corrupted").

(def http ((import "std/http")))
(def telemetry ((import "std/telemetry")))

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
