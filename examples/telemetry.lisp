#!/usr/bin/env elle

# Telemetry — OpenTelemetry metrics with OTLP/HTTP export
#
# Demonstrates:
#   telemetry:meter              — create a meter with OTLP endpoint
#   telemetry:counter            — monotonic counter instrument
#   telemetry:gauge              — last-value gauge instrument
#   telemetry:histogram          — distribution with explicit buckets
#   telemetry:add / record / set — recording observations
#   telemetry:time               — automatic latency measurement
#   telemetry:flush              — manual export to collector
#   telemetry:shutdown           — graceful teardown
#
# A mock OTLP collector captures exported payloads for verification.
# The toy application simulates an HTTP service processing requests
# with varying latencies and tracks revenue, connections, and errors.

(def http ((import-file "./lib/http.lisp")))
(def telemetry ((import-file "lib/telemetry.lisp")))


# ── Mock OTLP collector ──────────────────────────────────────────────

(def received @[])

(defn collector-handler [request]
  (push received (json-parse request:body))
  (http:respond 200 ""))

(def listener (tcp/listen "127.0.0.1" 0))
(def collector-port (integer (get (string/split (port/path listener) ":") 1)))
(def collector-url (string "http://127.0.0.1:" collector-port "/v1/metrics"))
(def server (ev/spawn (fn [] (http:serve listener collector-handler))))
(print "  collector on port ") (println collector-port)


# ── Create the meter ─────────────────────────────────────────────────

(def meter (telemetry:meter "order-service"
  :endpoint collector-url
  :interval 9999
  :resource {"deployment.environment" "staging"
             "host.name" "web-01"}))


# ── Register instruments ─────────────────────────────────────────────

(def http-requests
  (telemetry:counter meter "http.server.request.count"
    :unit "1"
    :description "Total inbound HTTP requests"))

(def http-latency
  (telemetry:histogram meter "http.server.request.duration"
    :unit "s"
    :description "Request processing time"
    :boundaries [0.005 0.01 0.025 0.05 0.1 0.25 0.5 1.0]))

(def db-connections
  (telemetry:gauge meter "db.client.connections"
    :unit "1"
    :description "Active database connections"))

(def order-revenue
  (telemetry:counter meter "orders.revenue"
    :unit "USD"
    :description "Cumulative order revenue"))


# ── Simulate application traffic ─────────────────────────────────────

(defn simulate-request [method path status price]
  "Simulate handling one HTTP request."
  (let [[attrs {"http.method" method
                "http.route"  path
                "http.status" status}]]
    (telemetry:add http-requests 1 :attributes attrs)
    (telemetry:time http-latency
      (fn [] (ev/sleep (/ (+ 1 (mod (* status 7) 50)) 1000.0)))
      :attributes attrs)
    (when price
      (telemetry:add order-revenue price
        :attributes {"currency" "USD" "region" "us-east"}))))

(simulate-request "GET"  "/api/orders"     200 nil)
(simulate-request "POST" "/api/orders"     201 49.99)
(simulate-request "GET"  "/api/orders/123" 200 nil)
(simulate-request "POST" "/api/orders"     201 129.50)
(simulate-request "GET"  "/api/orders"     200 nil)
(simulate-request "GET"  "/api/health"     200 nil)
(simulate-request "GET"  "/api/orders/999" 404 nil)
(simulate-request "POST" "/api/orders"     201 24.95)
(println "  simulated 8 requests")


# ── Gauge: connection pool over time ─────────────────────────────────

(telemetry:set db-connections 2 :attributes {"db.system" "postgresql"})
(telemetry:set db-connections 5 :attributes {"db.system" "postgresql"})
(telemetry:set db-connections 3 :attributes {"db.system" "postgresql"})
(println "  db pool: 2 -> 5 -> 3")


# ── Flush to the mock collector ──────────────────────────────────────

(telemetry:flush meter)
(ev/sleep 0.05)

(print "  collector received ") (print (length received)) (println " export(s)")
(assert (>= (length received) 1) "collector got at least one export")


# ── Inspect the exported payload ─────────────────────────────────────

(def export (get received 0))
(def exported-rm (get (get export "resourceMetrics") 0))
(def resource-attrs (get (get exported-rm "resource") "attributes"))
(def exported-scope (get (get (get exported-rm "scopeMetrics") 0) "metrics"))

(print "  resource attributes: ") (println (length resource-attrs))
(each m in exported-scope
  (print "    ") (print (get m "name"))
  (cond
    ((has? m "sum")       (println " (sum)"))
    ((has? m "gauge")     (println " (gauge)"))
    ((has? m "histogram") (println " (histogram)"))))

(assert (= (length exported-scope) 4) "all four metrics exported")


# ── Second flush: points cleared, nothing new ────────────────────────

(def count-before (length received))
(telemetry:flush meter)
(ev/sleep 0.05)
(assert (= (length received) count-before) "no duplicate after flush")
(println "  second flush: no duplicate (points cleared)")


# ── Incremental export after new observations ────────────────────────

(simulate-request "DELETE" "/api/orders/123" 204 nil)
(telemetry:set db-connections 4 :attributes {"db.system" "postgresql"})
(telemetry:flush meter)
(ev/sleep 0.05)
(assert (> (length received) count-before) "new data triggers export")
(println "  incremental export after new activity")


# ── Verify histogram bucketing ───────────────────────────────────────

(var hist-metric nil)
(each m in exported-scope
  (when (= (get m "name") "http.server.request.duration")
    (assign hist-metric m)))

(assert (not (nil? hist-metric)) "histogram was exported")
(def hist-data (get hist-metric "histogram"))
(def total-obs
  (fold (fn [acc dp] (+ acc (integer (get dp "count"))))
    0
    (get hist-data "dataPoints")))
(print "  histogram observations: ") (println total-obs)
(assert (= total-obs 8) "histogram captured all 8 requests")

(def sample-dp (get (get hist-data "dataPoints") 0))
(print "  sample bucket counts: ") (println (get sample-dp "bucketCounts"))
(assert (= (length (get sample-dp "bucketCounts")) 9) "8 boundaries + overflow")


# ── JSON round-trip ──────────────────────────────────────────────────

(def json-str (json-serialize export))
(def reparsed (json-parse json-str))
(assert (= (length (get reparsed "resourceMetrics")) 1) "JSON round-trips")
(println "  OTLP JSON round-trip: ok")


# ── Shutdown + teardown ──────────────────────────────────────────────

(telemetry:shutdown meter)
(ev/abort server)
(port/close listener)

(println "")
(println "all telemetry passed.")
