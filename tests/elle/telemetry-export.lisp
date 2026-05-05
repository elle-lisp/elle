#!/usr/bin/env elle
(elle/epoch 10)

# tests/elle/telemetry-export.lisp — OTLP export integration tests
#
# Verifies that telemetry:flush delivers payloads over HTTP to a
# mock collector.  Each section isolates one variable so a hang
# pinpoints the exact failure boundary.
#
# Run: elle tests/elle/telemetry-export.lisp

(def http ((import-file "./lib/http.lisp")))
(def telemetry ((import-file "lib/telemetry.lisp")))


# ── Mock collector ────────────────────────────────────────────────────
#
# Tiny HTTP server that stashes POST bodies and responds 200.

(def received @[])

(defn collector-handler [request]
  (push received request:body)
  (http:respond 200 "ok"))

(let [listener (tcp/listen "127.0.0.1" 0)]
  (let* [addr (port/path listener)
         port-num (parse-int (get (string/split addr ":") 1))
         url (string "http://127.0.0.1:" port-num "/v1/metrics")]
    (def server (ev/spawn (fn [] (http:serve listener collector-handler))))


    # ── 1. Direct http:post to collector ──────────────────────────────

    (let [r (http:post url "hello")]
      (assert (= r:status 200) "direct http:post works")
      (assert (= (length received) 1) "collector received direct post"))
    (println "  1. direct http:post: ok")


    # ── 2. http:post with JSON body ───────────────────────────────────

    (let [r (http:post url (json-serialize {"test" true})
                       :headers {:content-type "application/json"})]
      (assert (= r:status 200) "JSON http:post works")
      (assert (= (length received) 2) "collector received JSON post"))
    (println "  2. http:post with JSON body: ok")


    # ── 3. http:post with telemetry-sized JSON body ───────────────────

    (def meter-stub
      @{:service "test"
        :endpoint url
        :interval 9999
        :resource {"service.name" "test"}
        :headers {}
        :metrics @[]
        :start-nanos 1000000000000000000
        :shutdown? false
        :exporter nil
        :temporality :cumulative
        :enabled true
        :on-export nil})

    (def c (telemetry:counter meter-stub "req"))
    (telemetry:add c 1 :attributes {"method" "GET"})

    (def payload (telemetry:build-payload meter-stub))
    (def body (json-serialize payload))

    (let [r (http:post url body :headers {:content-type "application/json"})]
      (assert (= r:status 200) "telemetry-sized JSON body works")
      (assert (= (length received) 3) "collector received telemetry body"))
    (println "  3. http:post with telemetry JSON body: ok")


    # ── 4. telemetry:flush with manual meter (no background fiber) ────

    # Reset received for clean counting
    (while (not (empty? received)) (pop received))

    (telemetry:flush meter-stub)
    (ev/sleep 0.05)
    (assert (= (length received) 1) "telemetry:flush delivered payload")
    (println "  4. telemetry:flush (manual meter): ok")


    # ── 5. telemetry:flush with real meter (background fiber) ─────────

    (while (not (empty? received)) (pop received))

    (def meter (telemetry:meter "test-svc" :endpoint url :interval 9999))
    (def counter (telemetry:counter meter "http.requests"))
    (telemetry:add counter 1 :attributes {"method" "GET"})

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (= (length received) 1) "telemetry:flush (real meter) delivered")
    (println "  5. telemetry:flush (real meter): ok")


    # ── 6. Flush after telemetry:time (the previously-hanging case) ───

    (while (not (empty? received)) (pop received))

    (def latency
      (telemetry:histogram meter "latency" :unit "s"
                           :boundaries [0.01 0.05 0.1 0.5 1.0]))
    (telemetry:time latency (fn [] (ev/sleep 0.001)) :attributes {"op" "test"})

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "flush after telemetry:time delivered")
    (println "  6. telemetry:flush after telemetry:time: ok")


    # ── 6b. Flush after many telemetry:time calls ──────────────────────

    (while (not (empty? received)) (pop received))

    (def @i 0)
    (while (< i 8)
      (telemetry:time latency (fn [] (ev/sleep 0.001))
                      :attributes {"op" (string "req-" i)})
      (assign i (+ i 1)))

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "flush after 8x telemetry:time delivered")
    (println "  6b. telemetry:flush after 8x telemetry:time: ok")


    # ── 6c. Full simulation: 4 instruments, mixed recording, then flush ─

    (while (not (empty? received)) (pop received))

    (def http-requests (telemetry:counter meter "sim.requests" :unit "1"))
    (def order-revenue (telemetry:counter meter "sim.revenue" :unit "USD"))
    (def db-conns (telemetry:gauge meter "sim.connections" :unit "1"))

    (defn simulate-request [method path status price]
      (let [attrs {"http.method" method "http.route" path "http.status" status}]
        (telemetry:add http-requests 1 :attributes attrs)
        (telemetry:time latency
                        (fn [] (ev/sleep (/ (+ 1 (mod (* status 7) 50)) 1000.0)))
                        :attributes attrs)
        (when price
          (telemetry:add order-revenue price
                         :attributes {"currency" "USD" "region" "us-east"}))))

    (simulate-request "GET" "/api/orders" 200 nil)
    (simulate-request "POST" "/api/orders" 201 49.99)
    (simulate-request "GET" "/api/orders/123" 200 nil)
    (simulate-request "POST" "/api/orders" 201 129.50)
    (simulate-request "GET" "/api/orders" 200 nil)
    (simulate-request "GET" "/api/health" 200 nil)
    (simulate-request "GET" "/api/orders/999" 404 nil)
    (simulate-request "POST" "/api/orders" 201 24.95)

    (telemetry:set db-conns 2 :attributes {"db.system" "postgresql"})
    (telemetry:set db-conns 5 :attributes {"db.system" "postgresql"})
    (telemetry:set db-conns 3 :attributes {"db.system" "postgresql"})

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "full simulation flush delivered")
    (println "  6c. full simulation flush: ok")


    # ── 6d. build-payload then flush (example pattern) ──────────────────

    (while (not (empty? received)) (pop received))

    # Record fresh data
    (telemetry:add http-requests 1 :attributes {"method" "DELETE"})
    (telemetry:time latency (fn [] (ev/sleep 0.001)) :attributes {"op" "del"})

    # Inspect payload first (like the example does)
    (def pre-payload (telemetry:build-payload meter))
    (assert (not (nil? pre-payload)) "pre-flush payload exists")

    # Then flush
    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "flush after build-payload works")
    (println "  6d. build-payload then flush: ok")


    # ── 6e. Exact example sequence ──────────────────────────────────────

    (while (not (empty? received)) (pop received))

    (simulate-request "GET" "/api/orders" 200 nil)
    (simulate-request "POST" "/api/orders" 201 49.99)
    (simulate-request "GET" "/api/orders/123" 200 nil)
    (simulate-request "POST" "/api/orders" 201 129.50)
    (simulate-request "GET" "/api/orders" 200 nil)
    (simulate-request "GET" "/api/health" 200 nil)
    (simulate-request "GET" "/api/orders/999" 404 nil)
    (simulate-request "POST" "/api/orders" 201 24.95)

    (telemetry:set db-conns 2 :attributes {"db.system" "postgresql"})
    (telemetry:set db-conns 5 :attributes {"db.system" "postgresql"})
    (telemetry:set db-conns 3 :attributes {"db.system" "postgresql"})

    (def ex-payload (telemetry:build-payload meter))
    (def ex-rm (get (get ex-payload "resourceMetrics") 0))
    (def ex-scope (get (get (get ex-rm "scopeMetrics") 0) "metrics"))
    (assert (>= (length ex-scope) 4) "payload has instruments")

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "exact example sequence flush delivered")
    (println "  6e. exact example sequence: ok")


    # ── 6f. simulate + inspect payload + flush (was known failure) ──────
    #
    # Previously crashed with "Upvalue index N out of bounds" due to
    # the emitter not saving/restoring current_func_num_locals around
    # nested function emission.  Fixed in #673.

    (while (not (empty? received)) (pop received))

    (simulate-request "GET" "/api/orders" 200 nil)
    (simulate-request "POST" "/api/orders" 201 49.99)
    (simulate-request "GET" "/api/orders/123" 200 nil)
    (simulate-request "POST" "/api/orders" 201 129.50)
    (simulate-request "GET" "/api/orders" 200 nil)
    (simulate-request "GET" "/api/health" 200 nil)
    (simulate-request "GET" "/api/orders/999" 404 nil)
    (simulate-request "POST" "/api/orders" 201 24.95)

    (telemetry:set db-conns 2 :attributes {"db.system" "postgresql"})
    (telemetry:set db-conns 5 :attributes {"db.system" "postgresql"})
    (telemetry:set db-conns 3 :attributes {"db.system" "postgresql"})

    (def f-payload (telemetry:build-payload meter))
    (def f-rm (get (get f-payload "resourceMetrics") 0))
    (def f-scope (get (get (get f-rm "scopeMetrics") 0) "metrics"))
    (assert (>= (length f-scope) 4) "6f payload has instruments")

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (>= (length received) 1) "6f flush delivered")
    (println "  6f. simulate + inspect + flush (was known failure): ok")


    # ── 7. Second flush sends nothing (points cleared) ────────────────

    (while (not (empty? received)) (pop received))

    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (= (length received) 0) "second flush sends nothing")
    (println "  7. idempotent flush: ok")


    # ── 8. Incremental export after new observations ──────────────────

    (telemetry:add counter 1 :attributes {"method" "POST"})
    (telemetry:flush meter)
    (ev/sleep 0.05)
    (assert (= (length received) 1) "incremental flush delivered")
    (println "  8. incremental flush: ok")


    # ── Teardown ──────────────────────────────────────────────────────

    (telemetry:shutdown meter)
    (ev/abort server)
    (port/close listener)

    (println "")
    (println "all telemetry-export tests passed.")))
