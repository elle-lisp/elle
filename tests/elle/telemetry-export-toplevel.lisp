#!/usr/bin/env elle
(elle/epoch 7)

# tests/elle/telemetry-export-toplevel.lisp
#
# Reproduces the exact top-level structure of examples/telemetry.lisp
# to isolate whether the flush hang is scope-dependent.

(def http ((import-file "./lib/http.lisp")))
(def telemetry ((import-file "lib/telemetry.lisp")))

(def received @[])

(defn collector-handler [request]
  (push received (json-parse request:body))
  (http:respond 200 ""))

(def listener (tcp/listen "127.0.0.1" 0))
(def collector-port (integer (get (string/split (port/path listener) ":") 1)))
(def collector-url (string "http://127.0.0.1:" collector-port "/v1/metrics"))
(def server (ev/spawn (fn [] (http:serve listener collector-handler))))

(def meter (telemetry:meter "test" :endpoint collector-url :interval 9999))

(def c (telemetry:counter meter "req" :unit "1"))
(def h (telemetry:histogram meter "lat" :unit "s"
  :boundaries [0.005 0.01 0.025 0.05 0.1 0.25 0.5 1.0]))
(def g (telemetry:gauge meter "conns" :unit "1"))
(def rev (telemetry:counter meter "rev" :unit "USD"))

(defn sim [method path status price]
  (let [attrs {"m" method "p" path "s" status}]
    (telemetry:add c 1 :attributes attrs)
    (telemetry:time h
      (fn [] (ev/sleep 0.001))
      :attributes attrs)
    (when price
      (telemetry:add rev price :attributes {"cur" "USD"}))))

(println "  simulating...")
(sim "GET"  "/a" 200 nil)
(sim "POST" "/a" 201 49.99)
(sim "GET"  "/b" 200 nil)
(sim "POST" "/a" 201 129.50)
(sim "GET"  "/a" 200 nil)
(sim "GET"  "/c" 200 nil)
(sim "GET"  "/d" 404 nil)
(sim "POST" "/a" 201 24.95)
(println "  simulated")

(telemetry:set g 3 :attributes {"db" "pg"})

(println "  flushing...")
(telemetry:flush meter)
(ev/sleep 0.05)
(println "  flushed")

(assert (>= (length received) 1) "collector received export")
(print "  received: ") (println (length received))

(telemetry:shutdown meter)
(ev/abort server)
(port/close listener)
(println "  done")
(println "")
(println "all telemetry-export-toplevel tests passed.")
