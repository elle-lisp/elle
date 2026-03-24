#!/usr/bin/env elle

# tests/elle/telemetry.lisp — Unit tests for lib/telemetry.lisp
#
# Tests the OTLP JSON payload construction, attribute encoding,
# and metric aggregation without requiring a running collector.
# Run: ./target/debug/elle tests/elle/telemetry.lisp

(def telemetry ((import-file "lib/telemetry.lisp")))

(println "=== telemetry: attribute encoding ===")

# nil attributes => empty array
(assert (= (telemetry:encode-attributes nil) []) "nil attrs => []")

# string value
(let [[encoded (telemetry:encode-attributes {"method" "GET"})]]
  (assert (= (length encoded) 1) "one attribute")
  (let [[kv (get encoded 0)]]
    (assert (= (get kv "key") "method") "attr key")
    (assert (= (get (get kv "value") "stringValue") "GET") "attr string value")))

# integer value
(let [[encoded (telemetry:encode-attributes {"code" 200})]]
  (let [[kv (get encoded 0)]]
    (assert (= (get (get kv "value") "intValue") "200") "attr int value")))

# float value
(let [[encoded (telemetry:encode-attributes {"latency" 1.5})]]
  (let [[kv (get encoded 0)]]
    (assert (= (get (get kv "value") "doubleValue") 1.5) "attr float value")))

# boolean value
(let [[encoded (telemetry:encode-attributes {"ok" true})]]
  (let [[kv (get encoded 0)]]
    (assert (= (get (get kv "value") "boolValue") true) "attr bool value")))

(println "  attribute encoding: ok")


(println "=== telemetry: payload construction ===")

# Build a meter without starting the export loop (test the data model only).
# We construct the internal struct directly to avoid needing ev/run.

(def test-meter
  @{"service"     "test-svc"
    "endpoint"    "http://localhost:9999"
    "interval"    9999
    "resource"    {"service.name" "test-svc" "env" "test"}
    "headers"     {}
    "metrics"     @[]
    "start-nanos" 1000000000000000000
    "shutdown?"   false
    "exporter"    nil})

# Create instruments manually (no export loop)
(def counter
  @{"type"        "sum"
    "name"        "test.requests"
    "unit"        "1"
    "description" "Test counter"
    "meter"       test-meter
    "monotonic"   true
    "points"      @[]})
(push (get test-meter "metrics") counter)

(def gauge
  @{"type"        "gauge"
    "name"        "test.temperature"
    "unit"        "C"
    "description" "Test gauge"
    "meter"       test-meter
    "points"      @[]})
(push (get test-meter "metrics") gauge)

# Record some points
(push (get counter "points")
  {"value" 5 "attributes" {"method" "GET"} "time" 1000000001000000000})
(push (get counter "points")
  {"value" 3 "attributes" {"method" "GET"} "time" 1000000002000000000})
(push (get counter "points")
  {"value" 1 "attributes" {"method" "POST"} "time" 1000000001500000000})

(push (get gauge "points")
  {"value" 22.5 "attributes" {} "time" 1000000003000000000})

# Build the payload
(def payload (telemetry:build-payload test-meter))
(assert (not (nil? payload)) "payload is not nil")

# Check top-level structure
(def resource-metrics (get payload "resourceMetrics"))
(assert (= (length resource-metrics) 1) "one resourceMetrics entry")

(def rm (get resource-metrics 0))
(def resource (get rm "resource"))
(def resource-attrs (get resource "attributes"))
(assert (>= (length resource-attrs) 1) "resource has attributes")

(def scope-metrics (get rm "scopeMetrics"))
(assert (= (length scope-metrics) 1) "one scopeMetrics entry")

(def sm (get scope-metrics 0))
(def scope (get sm "scope"))
(assert (= (get scope "name") "elle-telemetry") "scope name")
(assert (= (get scope "version") "0.1.0") "scope version")

(def metrics (get sm "metrics"))
(assert (= (length metrics) 2) "two metrics")

# Check counter metric
(def counter-metric (get metrics 0))
(assert (= (get counter-metric "name") "test.requests") "counter name")
(assert (= (get counter-metric "unit") "1") "counter unit")
(def sum-data (get counter-metric "sum"))
(assert (not (nil? sum-data)) "counter has sum data")
(assert (= (get sum-data "isMonotonic") true) "counter is monotonic")
(assert (= (get sum-data "aggregationTemporality") 2) "cumulative temporality")

(def sum-points (get sum-data "dataPoints"))
# Should have 2 groups: GET (5+3=8) and POST (1)
(assert (= (length sum-points) 2) "counter has 2 attribute groups")

# Check gauge metric
(def gauge-metric (get metrics 1))
(assert (= (get gauge-metric "name") "test.temperature") "gauge name")
(def gauge-data (get gauge-metric "gauge"))
(assert (not (nil? gauge-data)) "gauge has gauge data")
(def gauge-points (get gauge-data "dataPoints"))
(assert (= (length gauge-points) 1) "gauge has 1 data point")
(assert (= (get (get gauge-points 0) "asDouble") 22.5) "gauge value")

(println "  payload construction: ok")


(println "=== telemetry: JSON round-trip ===")

# Verify the payload serializes to valid JSON and back
(def json-str (json-serialize payload))
(assert (string? json-str) "serializes to string")
(assert (string-contains? json-str "resourceMetrics") "contains resourceMetrics")
(assert (string-contains? json-str "test.requests") "contains metric name")

(def reparsed (json-parse json-str))
(assert (not (nil? reparsed)) "parses back")
(assert (= (length (get reparsed "resourceMetrics")) 1) "round-trip structure intact")

(println "  JSON round-trip: ok")


(println "=== telemetry: histogram encoding ===")

(def hist
  @{"type"        "histogram"
    "name"        "test.latency"
    "unit"        "s"
    "description" "Test histogram"
    "meter"       test-meter
    "boundaries"  [0.01 0.05 0.1 0.5 1.0]
    "points"      @[]})
(push (get test-meter "metrics") hist)

# Record observations across buckets
(push (get hist "points")
  {"value" 0.005 "attributes" {} "time" 1000000004000000000})  # bucket 0 (<=0.01)
(push (get hist "points")
  {"value" 0.03  "attributes" {} "time" 1000000004100000000})  # bucket 1 (<=0.05)
(push (get hist "points")
  {"value" 0.07  "attributes" {} "time" 1000000004200000000})  # bucket 2 (<=0.1)
(push (get hist "points")
  {"value" 2.0   "attributes" {} "time" 1000000004300000000})  # overflow bucket

(def payload2 (telemetry:build-payload test-meter))
(def metrics2 (get (get (get payload2 "resourceMetrics") 0) "scopeMetrics"))
(def all-metrics (get (get metrics2 0) "metrics"))

# Find the histogram metric (it's the third one)
(def hist-metric (get all-metrics 2))
(assert (= (get hist-metric "name") "test.latency") "histogram name")
(def hist-data (get hist-metric "histogram"))
(assert (not (nil? hist-data)) "histogram has histogram data")
(assert (= (get hist-data "aggregationTemporality") 2) "histogram cumulative")

(def hist-points (get hist-data "dataPoints"))
(assert (= (length hist-points) 1) "histogram has 1 attribute group")
(def hp (get hist-points 0))
(assert (= (get hp "count") "4") "histogram count")
(assert (= (length (get hp "bucketCounts")) 6) "6 bucket counts (5 bounds + overflow)")
(assert (= (length (get hp "explicitBounds")) 5) "5 explicit bounds")

(println "  histogram encoding: ok")


(println "=== telemetry: empty payload ===")

# A meter with no recorded points should produce nil payload
(def empty-meter
  @{"service"     "empty"
    "endpoint"    "http://localhost:9999"
    "interval"    9999
    "resource"    {"service.name" "empty"}
    "headers"     {}
    "metrics"     @[]
    "start-nanos" 1000000000000000000
    "shutdown?"   false
    "exporter"    nil})
(assert (nil? (telemetry:build-payload empty-meter)) "empty meter => nil payload")

# Meter with instruments but no points
(def empty-counter
  @{"type" "sum" "name" "x" "unit" "1" "description" ""
    "meter" empty-meter "monotonic" true "points" @[]})
(push (get empty-meter "metrics") empty-counter)
(assert (nil? (telemetry:build-payload empty-meter)) "no points => nil payload")

(println "  empty payload: ok")


(println "")
(println "All telemetry tests passed.")
