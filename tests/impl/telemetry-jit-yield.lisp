#!/usr/bin/env elle
(elle/epoch 12)
# audited: 2026-09-29
# A compiled closure that yields through a call rebuilds its environment from its own local count.
# docs/impl/jit.md
#
# Each call site's metadata records `num_locals` of the function that holds
# the call (`current_func_num_locals`, src/lir/emit/mod.rs), and the JIT's
# yield-through-call path rebuilds the environment from it. `sim` holds a
# nested closure, the thunk it passes to telemetry:time, so a count taken from
# the nested closure is the wrong one.
#
# The counter-factual: call-site metadata that records the nested closure's
# count rebuilds the environment too small once `sim` is compiled, and the
# resumed body fails with "Upvalue index 7 out of bounds (env size: 4)". The
# default build compiles after 10 calls, so the file makes 16.

(def http ((import-file "./lib/http.lisp")))
(def telemetry ((import-file "lib/telemetry.lisp")))

(def received @[])

(defn collector-handler [request]
  (push received request:body)
  (http:respond 200 "ok"))

(let [listener (tcp/listen "127.0.0.1" 0)]
  (let* [addr (port/path listener)
         port-num (parse-int (get (string/split addr ":") 1))
         url (string "http://127.0.0.1:" port-num "/v1/metrics")]
    (def server (ev/spawn (fn [] (http:serve listener collector-handler))))
    (def meter (telemetry:meter "t" :endpoint url :interval 9999))
    (def lat
      (telemetry:histogram meter "lat" :unit "s"
                           :boundaries [0.01 0.05 0.1 0.5 1.0]))
    (def req-c (telemetry:counter meter "reqs" :unit "1"))
    (def rev-c (telemetry:counter meter "rev" :unit "USD"))
    (def gauge (telemetry:gauge meter "conns" :unit "1"))

    (defn sim [method path status price]
      (let [attrs {"m" method "p" path "s" status}]
        (telemetry:add req-c 1 :attributes attrs)
        (telemetry:time lat
                        (fn [] (ev/sleep (/ (+ 1 (mod (* status 7) 50)) 1000.0)))
                        :attributes attrs)
        (when price (telemetry:add rev-c price :attributes {"cur" "USD"}))))

    # 16 sim calls, past the default JIT threshold of 10.
    (def @i 0)
    (while (< i 16)
      (sim "GET" "/a" 200 nil)
      (assign i (+ i 1)))
    (println "  1. 16 yielding calls past JIT threshold: ok")

    # Flush and verify export works after JIT compilation
    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "flush after JIT calls delivered")
    (println "  2. flush after JIT calls: ok")

    # Second batch — exercises JIT-compiled code paths
    (while (not (empty? received)) (pop received))
    (sim "GET" "/a" 200 nil)
    (sim "POST" "/b" 201 49.99)
    (sim "GET" "/c" 200 nil)
    (sim "POST" "/d" 201 129.50)
    (telemetry:set gauge 3 :attributes {"db" "pg"})

    # Build the payload and inspect it, as tests/lang/telemetry-export.lisp does.
    (def payload (telemetry:build-payload meter))
    (def rm (get (get payload "resourceMetrics") 0))
    (def scope (get (get rm "scopeMetrics") 0))
    (def metrics (get scope "metrics"))
    (assert (>= (length metrics) 3) "payload has instruments after JIT")

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "post-JIT sim+inspect+flush delivered")
    (println "  3. sim+inspect+flush after JIT: ok")

    (telemetry:shutdown meter)
    (ev/abort server)
    (port/close listener)
    (println "")
    (println "all telemetry-jit-yield tests passed.")))
