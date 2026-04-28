#!/usr/bin/env elle
(elle/epoch 9)

# tests/elle/telemetry.lisp — Unit tests for lib/telemetry.lisp
#
# Tests OTLP JSON payload construction, attribute encoding,
# metric aggregation, and new v0.2 features without a running collector.
# Run: elle tests/elle/telemetry.lisp

(def telemetry ((import-file "lib/telemetry.lisp")))

(println "=== telemetry: attribute encoding ===")

# nil attributes => empty array
(assert (= (telemetry:encode-attributes nil) []) "nil attrs => []")

# string value
(let [encoded (telemetry:encode-attributes {"method" "GET"})]
  (assert (= (length encoded) 1) "one attribute")
  (let [kv (get encoded 0)]
    (assert (= (get kv "key") "method") "attr key")
    (assert (= (get (get kv "value") "stringValue") "GET") "attr string value")))

# integer value
(let [encoded (telemetry:encode-attributes {"code" 200})]
  (let [kv (get encoded 0)]
    (assert (= (get (get kv "value") "intValue") "200") "attr int value")))

# float value
(let [encoded (telemetry:encode-attributes {"latency" 1.5})]
  (let [kv (get encoded 0)]
    (assert (= (get (get kv "value") "doubleValue") 1.5) "attr float value")))

# boolean value
(let [encoded (telemetry:encode-attributes {"ok" true})]
  (let [kv (get encoded 0)]
    (assert (= (get (get kv "value") "boolValue") true) "attr bool value")))

(println "  attribute encoding: ok")


(println "=== telemetry: payload construction (pre-aggregated) ===")

# Build a meter struct directly — no export loop needed for unit tests.
(def test-meter
  @{:service "test-svc"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "test-svc" "env" "test"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled true
    :on-export nil})

# Create instruments via constructor functions
(def counter
  (telemetry:counter test-meter "test.requests" :unit "1"
    :description "Test counter"))
(def gauge
  (telemetry:gauge test-meter "test.temperature" :unit "C"
    :description "Test gauge"))

# Record some points via the public API
(telemetry:add counter 5 :attributes {"method" "GET"})
(telemetry:add counter 3 :attributes {"method" "GET"})
(telemetry:add counter 1 :attributes {"method" "POST"})

(telemetry:set gauge 22.5)

# Build the payload (non-destructive peek)
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
(assert (= (get scope "version") "0.2.0") "scope version")

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

# Should have 2 groups: GET (5+3=8) and POST (1)
(def sum-points (get sum-data "dataPoints"))
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

(def json-str (json-serialize payload))
(assert (string? json-str) "serializes to string")
(assert (string-contains? json-str "resourceMetrics") "contains resourceMetrics")
(assert (string-contains? json-str "test.requests") "contains metric name")

(def reparsed (json-parse json-str))
(assert (not (nil? reparsed)) "parses back")
(assert (= (length (get reparsed "resourceMetrics")) 1)
  "round-trip structure intact")

(println "  JSON round-trip: ok")


(println "=== telemetry: histogram encoding ===")

(def hist
  (telemetry:histogram test-meter "test.latency" :unit "s"
    :description "Test histogram" :boundaries [0.01 0.05 0.1 0.5 1.0]))

# Record observations across buckets
(telemetry:record hist 0.005)  # bucket 0 (<=0.01)
(telemetry:record hist 0.03)  # bucket 1 (<=0.05)
(telemetry:record hist 0.07)  # bucket 2 (<=0.1)
(telemetry:record hist 2.0)  # overflow bucket

(def payload2 (telemetry:build-payload test-meter))
(def metrics2 (get (get (get payload2 "resourceMetrics") 0) "scopeMetrics"))
(def all-metrics (get (get metrics2 0) "metrics"))

# Find the histogram
(def hist-metric (get all-metrics 2))
(assert (= (get hist-metric "name") "test.latency") "histogram name")
(def hist-data (get hist-metric "histogram"))
(assert (not (nil? hist-data)) "histogram has histogram data")
(assert (= (get hist-data "aggregationTemporality") 2) "histogram cumulative")

(def hist-points (get hist-data "dataPoints"))
(assert (= (length hist-points) 1) "histogram has 1 attribute group")
(def hp (get hist-points 0))
(assert (= (get hp "count") "4") "histogram count")
(assert (= (length (get hp "bucketCounts")) 6)
  "6 bucket counts (5 bounds + overflow)")
(assert (= (length (get hp "explicitBounds")) 5) "5 explicit bounds")

(println "  histogram encoding: ok")


(println "=== telemetry: empty payload ===")

(def empty-meter
  @{:service "empty"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "empty"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled true
    :on-export nil})
(assert (nil? (telemetry:build-payload empty-meter))
  "empty meter => nil payload")

# Meter with instruments but no observations
(telemetry:counter empty-meter "x" :unit "1")
(assert (nil? (telemetry:build-payload empty-meter))
  "no observations => nil payload")

(println "  empty payload: ok")


(println "=== telemetry: isMonotonic from struct ===")

(def up-down-meter
  @{:service "ud"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "ud"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled true
    :on-export nil})

(def up-down
  (telemetry:counter up-down-meter "conns" :unit "1" :monotonic false))
(telemetry:add up-down 3)
(telemetry:add up-down (- 0 1))  # decrement

(def ud-payload (telemetry:build-payload up-down-meter))
(def ud-rm (get (get ud-payload "resourceMetrics") 0))
(def ud-metrics (get (get (get ud-rm "scopeMetrics") 0) "metrics"))
(def ud-sum (get ud-metrics 0))
(assert (= (get (get ud-sum "sum") "isMonotonic") false)
  "updown counter isMonotonic=false")

# Verify sum is 2 (3 + -1)
(def ud-dp (get (get (get ud-sum "sum") "dataPoints") 0))
(assert (= (get ud-dp "asDouble") 2.0) "updown sum is 2")

(println "  isMonotonic: ok")


(println "=== telemetry: gauge last-value semantics ===")

(def gauge-meter
  @{:service "g"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "g"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled true
    :on-export nil})

(def temp (telemetry:gauge gauge-meter "temperature" :unit "C"))
(telemetry:set temp 20)
(telemetry:set temp 25)
(telemetry:set temp 22)

# Only one aggregate entry (last value), not 3 accumulated points
(assert (= (length (pairs temp:aggregates)) 1) "gauge has 1 aggregate entry")
(def g-payload (telemetry:build-payload gauge-meter))
(def g-rm (get (get g-payload "resourceMetrics") 0))
(def g-metrics (get (get (get g-rm "scopeMetrics") 0) "metrics"))
(def g-metric (get g-metrics 0))
(def g-dp (get (get (get g-metric "gauge") "dataPoints") 0))
(assert (= (get g-dp "asDouble") 22.0) "gauge exports last value")

(println "  gauge last-value: ok")


(println "=== telemetry: SDK resource attributes ===")

(def sdk-meter
  @{:service "sdk-test"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "sdk-test"
               "telemetry.sdk.name" "elle-telemetry"
               "telemetry.sdk.version" "0.2.0"
               "telemetry.sdk.language" "elle"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled true
    :on-export nil})

(def sdk-counter (telemetry:counter sdk-meter "x"))
(telemetry:add sdk-counter 1)

(def sdk-payload (telemetry:build-payload sdk-meter))
(def sdk-attrs
  (get (get (get (get sdk-payload "resourceMetrics") 0) "resource") "attributes"))

# Find SDK attributes in the encoded array
(def @found-sdk-name false)
(def @found-sdk-version false)
(def @found-sdk-lang false)
(each kv in sdk-attrs
  (cond
    (= (get kv "key") "telemetry.sdk.name") (assign found-sdk-name true)
    (= (get kv "key") "telemetry.sdk.version") (assign found-sdk-version true)
    (= (get kv "key") "telemetry.sdk.language") (assign found-sdk-lang true)))

(assert found-sdk-name "resource has telemetry.sdk.name")
(assert found-sdk-version "resource has telemetry.sdk.version")
(assert found-sdk-lang "resource has telemetry.sdk.language")

(println "  SDK resource attributes: ok")


(println "=== telemetry: observe alias ===")

(def obs-meter
  @{:service "obs"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "obs"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled true
    :on-export nil})

(def obs-hist
  (telemetry:histogram obs-meter "lat" :unit "s" :boundaries [0.1 0.5 1.0]))
(telemetry:observe obs-hist 0.3)
(telemetry:observe obs-hist 0.7)

(def obs-payload (telemetry:build-payload obs-meter))
(assert (not (nil? obs-payload)) "observe recorded data")
(def obs-rm (get (get obs-payload "resourceMetrics") 0))
(def obs-metric (get (get (get (get obs-rm "scopeMetrics") 0) "metrics") 0))
(def obs-hdata (get obs-metric "histogram"))
(assert (= (get (get (get obs-hdata "dataPoints") 0) "count") "2")
  "observe: 2 observations")

(println "  observe alias: ok")


(println "=== telemetry: enabled? check ===")

(def disabled-meter
  @{:service "dis"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "dis"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :cumulative
    :enabled false
    :on-export nil})

(def dis-counter (telemetry:counter disabled-meter "x"))
(telemetry:add dis-counter 1)
(telemetry:add dis-counter 2)

(assert (= (length (pairs dis-counter:aggregates)) 0)
  "disabled meter: no aggregates")
(assert (nil? (telemetry:build-payload disabled-meter))
  "disabled meter: nil payload")
(assert (not (telemetry:enabled? disabled-meter)) "enabled? returns false")

(println "  enabled? check: ok")


(println "=== telemetry: DELTA temporality ===")

(def delta-meter
  @{:service "delta"
    :endpoint "http://localhost:9999"
    :interval 9999
    :resource {"service.name" "delta"}
    :headers {}
    :metrics @[]
    :start-nanos 1000000000000000000
    :shutdown? false
    :exporter nil
    :temporality :delta
    :enabled true
    :on-export nil})

(def delta-counter (telemetry:counter delta-meter "x"))
(telemetry:add delta-counter 1)

(def delta-payload (telemetry:build-payload delta-meter))
(def delta-rm (get (get delta-payload "resourceMetrics") 0))
(def delta-metric (get (get (get (get delta-rm "scopeMetrics") 0) "metrics") 0))
(assert (= (get (get delta-metric "sum") "aggregationTemporality") 1)
  "DELTA temporality code = 1")

(println "  DELTA temporality: ok")


(println "")
(println "All telemetry tests passed.")
