## lib/telemetry.lisp — OpenTelemetry metrics (OTLP/HTTP JSON export)
##
## Loaded via: (def telemetry ((import-file "lib/telemetry.lisp")))
## Usage:
##   (def meter (telemetry:meter "my-service" :endpoint "http://localhost:9090/api/v1/otlp/v1/metrics"))
##   (def reqs  (telemetry:counter meter "http.requests" :unit "1" :description "Total HTTP requests"))
##   (telemetry:add reqs 1 :attributes {"method" "GET" "path" "/api"})
##   (telemetry:shutdown meter)
##
## Exports OTLP/HTTP JSON — natively accepted by Prometheus (remote-write receiver),
## OpenTelemetry Collector, Grafana Alloy, and others.
##
## Metric types: counter, gauge, histogram.
## Export model: background fiber flushes accumulated data points at a configurable
## interval (default 60s). Manual flush via telemetry:flush.

(def http ((import-file "lib/http.lisp")))

# ============================================================================
# Timestamp helpers
# ============================================================================

(defn now-nanos []
  "Current wall-clock time as integer nanoseconds (OTLP wire format)."
  (let [[secs (clock/realtime)]]
    (integer (* secs 1_000_000_000))))

# ============================================================================
# Attribute encoding
# ============================================================================

(defn encode-value [v]
  "Encode an Elle value as an OTLP AnyValue."
  (cond
    ((integer? v) {"intValue" (string v)})
    ((float? v)   {"doubleValue" v})
    ((boolean? v) {"boolValue" v})
    (true         {"stringValue" (string v)})))

(defn encode-attributes [attrs]
  "Encode a struct of {string -> value} as OTLP KeyValue array.
   Returns an array, or an empty array if attrs is nil."
  (if (nil? attrs)
    []
    (begin
      (def result @[])
      (each [key val] in (pairs attrs)
        (push result {"key" (string key) "value" (encode-value val)}))
      (freeze result))))

# ============================================================================
# Metric data structures
#
# Each metric is a mutable struct holding:
#   :name :unit :description :type :meter
#   :data — mutable accumulation (type-specific)
# ============================================================================

(defn make-counter [meter name unit description]
  @{"type"        "sum"
    "name"        name
    "unit"        (or unit "1")
    "description" (or description "")
    "meter"       meter
    "monotonic"   true
    "points"      @[]})

(defn make-gauge [meter name unit description]
  @{"type"        "gauge"
    "name"        name
    "unit"        (or unit "1")
    "description" (or description "")
    "meter"       meter
    "points"      @[]})

(defn make-histogram [meter name unit description boundaries]
  @{"type"        "histogram"
    "name"        name
    "unit"        (or unit "ms")
    "description" (or description "")
    "meter"       meter
    "boundaries"  (or boundaries [0.005 0.01 0.025 0.05 0.075
                                   0.1 0.25 0.5 0.75 1.0 2.5
                                   5.0 7.5 10.0])
    "points"      @[]})

# ============================================================================
# Recording data points
# ============================================================================

(defn add-point [metric value attrs]
  "Append a numeric data point with attributes and timestamp."
  (push (get metric "points")
    {"value"      value
     "attributes" (or attrs {})
     "time"       (now-nanos)}))

(defn histogram-record [metric value attrs]
  "Record a histogram observation."
  (push (get metric "points")
    {"value"      value
     "attributes" (or attrs {})
     "time"       (now-nanos)}))

# ============================================================================
# OTLP JSON payload construction
# ============================================================================

(defn group-points-by-attrs [points]
  "Group data points by their serialized attributes key.
   Returns a list of (attrs . points) pairs."
  (def groups @{})
  (each pt in points
    (let [[key (json-serialize (get pt "attributes"))]]
      (unless (has? groups key)
        (put groups key @{"attrs" (get pt "attributes") "points" @[]}))
      (push (get (get groups key) "points") pt)))
  (values groups))

(defn encode-sum-datapoints [points start-nanos]
  "Encode counter data points: aggregate by attributes, emit cumulative sums."
  (def result @[])
  (each group in (group-points-by-attrs points)
    (let* [[attrs  (get group "attrs")]
           [pts    (get group "points")]
           [total  (fold (fn [acc p] (+ acc (get p "value"))) 0 pts)]
           [latest (fold (fn [acc p] (max acc (get p "time"))) 0 pts)]]
      (push result
        {"attributes"        (encode-attributes attrs)
         "startTimeUnixNano" (string start-nanos)
         "timeUnixNano"      (string latest)
         "asDouble"          (float total)})))
  (freeze result))

(defn encode-gauge-datapoints [points]
  "Encode gauge data points: last value per attribute set."
  (def result @[])
  (each group in (group-points-by-attrs points)
    (let* [[attrs  (get group "attrs")]
           [pts    (get group "points")]
           [last-pt (get pts (- (length pts) 1))]]
      (push result
        {"attributes"   (encode-attributes attrs)
         "timeUnixNano" (string (get last-pt "time"))
         "asDouble"     (float (get last-pt "value"))})))
  (freeze result))

(defn encode-histogram-datapoints [points boundaries start-nanos]
  "Encode histogram data points: bucket counts per attribute set."
  (def result @[])
  (each group in (group-points-by-attrs points)
    (let [[attrs (get group "attrs")]
          [pts   (get group "points")]]
      # Build bucket counts
      (def counts @[])
      (var i 0)
      (while (<= i (length boundaries))
        (push counts 0)
        (assign i (+ i 1)))
      (var total-sum 0.0)
      (var count 0)
      (var mn nil)
      (var mx nil)
      (var latest 0)
      (each pt in pts
        (let [[v (get pt "value")]
              [t (get pt "time")]]
          (assign total-sum (+ total-sum (float v)))
          (assign count (+ count 1))
          (assign mn (if (nil? mn) v (min mn v)))
          (assign mx (if (nil? mx) v (max mx v)))
          (assign latest (max latest t))
          # Find bucket
          (var found false)
          (var bi 0)
          (while (< bi (length boundaries))
            (when (<= v (get boundaries bi))
              (put counts bi (+ (get counts bi) 1))
              (assign found true)
              (break))
            (assign bi (+ bi 1)))
          (unless found
            # Overflow bucket (last)
            (let [[last-idx (length boundaries)]]
              (put counts last-idx (+ (get counts last-idx) 1))))))
      (push result
        {"attributes"        (encode-attributes attrs)
         "startTimeUnixNano" (string start-nanos)
         "timeUnixNano"      (string latest)
         "count"             (string count)
         "sum"               total-sum
         "min"               (float (or mn 0))
         "max"               (float (or mx 0))
         "explicitBounds"    boundaries
         "bucketCounts"      (map (fn [c] (string c)) (freeze counts))})))
  (freeze result))

(defn encode-metric [metric start-nanos]
  "Encode a single metric instrument as an OTLP Metric object."
  (let [[typ    (get metric "type")]
        [points (freeze (get metric "points"))]
        [name   (get metric "name")]]
    (if (empty? points)
      nil
      (begin
        (def base {"name"        name
                   "unit"        (get metric "unit")
                   "description" (get metric "description")})
        (case typ
          "sum"
            (put base "sum"
              {"dataPoints"            (encode-sum-datapoints points start-nanos)
               "aggregationTemporality" 2   # CUMULATIVE
               "isMonotonic"           true})

          "gauge"
            (put base "gauge"
              {"dataPoints" (encode-gauge-datapoints points)})

          "histogram"
            (put base "histogram"
              {"dataPoints"            (encode-histogram-datapoints
                                         points (get metric "boundaries") start-nanos)
               "aggregationTemporality" 2})
          base)))))

(defn build-export-payload [meter]
  "Build the full OTLP ExportMetricsServiceRequest JSON body."
  (let [[metrics (get meter "metrics")]
        [start   (get meter "start-nanos")]
        [res     (get meter "resource")]]
    (def encoded @[])
    (each m in metrics
      (let [[enc (encode-metric m start)]]
        (unless (nil? enc)
          (push encoded enc))))
    (if (empty? encoded)
      nil
      {"resourceMetrics"
       [{"resource"
         {"attributes" (encode-attributes res)}
         "scopeMetrics"
         [{"scope" {"name" "elle-telemetry" "version" "0.1.0"}
           "metrics" (freeze encoded)}]}]})))

# ============================================================================
# Export (HTTP POST)
# ============================================================================

(defn do-export [meter]
  "Export accumulated data points via OTLP/HTTP JSON. Clears points on success."
  (let [[payload (build-export-payload meter)]]
    (if (nil? payload)
      nil
      (let [[body     (json-serialize payload)]
            [endpoint (get meter "endpoint")]
            [headers  (or (get meter "headers") {})]]
        (let [[resp (http:post endpoint body
                      :headers (merge {:content-type "application/json"} headers))]]
          (if (and (>= resp:status 200) (< resp:status 300))
            (begin
              # Clear collected points on successful export
              (each m in (get meter "metrics")
                (let [[pts (get m "points")]]
                  (while (not (empty? pts))
                    (pop pts))))
              true)
            (begin
              (eprintln "telemetry: export failed: " resp:status " " (or resp:body ""))
              false)))))))

# ============================================================================
# Background export fiber
# ============================================================================

(defn start-export-loop [meter]
  "Spawn a background fiber that exports at the configured interval.
   Returns the fiber handle."
  (ev/spawn (fn []
    (let [[interval (get meter "interval")]]
      (forever
        (ev/sleep interval)
        (let [[[ok? _] (protect (do-export meter))]]
          (unless ok?
            (eprintln "telemetry: background export error")))
        (when (get meter "shutdown?")
          (break)))))))

# ============================================================================
# Meter (top-level registry)
# ============================================================================

(defn telemetry-meter [service-name &named endpoint interval resource headers]
  "Create a meter (metric registry + exporter).
   :endpoint — OTLP/HTTP endpoint URL (default: Prometheus OTLP receiver)
   :interval — export interval in seconds (default: 60)
   :resource — extra resource attributes struct
   :headers  — extra HTTP headers for export requests
   Returns a mutable struct."
  (let [[base-resource (merge {"service.name" service-name}
                              (or resource {}))]]
    (def meter
      @{"service"     service-name
        "endpoint"    (or endpoint "http://localhost:9090/api/v1/otlp/v1/metrics")
        "interval"    (or interval 60)
        "resource"    base-resource
        "headers"     (or headers {})
        "metrics"     @[]
        "start-nanos" (now-nanos)
        "shutdown?"   false
        "exporter"    nil})
    (put meter "exporter" (start-export-loop meter))
    meter))

# ============================================================================
# Instrument constructors
# ============================================================================

(defn telemetry-counter [meter name &named unit description]
  "Create a monotonic counter. Returns the instrument."
  (let [[c (make-counter meter name unit description)]]
    (push (get meter "metrics") c)
    c))

(defn telemetry-gauge [meter name &named unit description]
  "Create a gauge. Returns the instrument."
  (let [[g (make-gauge meter name unit description)]]
    (push (get meter "metrics") g)
    g))

(defn telemetry-histogram [meter name &named unit description boundaries]
  "Create a histogram. Returns the instrument.
   :boundaries — explicit bucket boundaries (default: HTTP latency buckets)."
  (let [[h (make-histogram meter name unit description boundaries)]]
    (push (get meter "metrics") h)
    h))

# ============================================================================
# Recording API
# ============================================================================

(defn telemetry-add [instrument value &named attributes]
  "Add a value to a counter or gauge."
  (add-point instrument value attributes))

(defn telemetry-record [instrument value &named attributes]
  "Record a value to a histogram."
  (histogram-record instrument value attributes))

(defn telemetry-set [instrument value &named attributes]
  "Set a gauge to a specific value (alias for add on gauge)."
  (add-point instrument value attributes))

# ============================================================================
# Lifecycle
# ============================================================================

(defn telemetry-flush [meter]
  "Force an immediate export of all accumulated data points."
  (do-export meter))

(defn telemetry-shutdown [meter]
  "Flush remaining data and stop the background exporter."
  (do-export meter)
  (put meter "shutdown?" true)
  (let [[exporter (get meter "exporter")]]
    (when exporter
      (ev/abort exporter)))
  nil)

# ============================================================================
# Convenience: timed operation
# ============================================================================

(defn telemetry-time [histogram thunk &named attributes]
  "Execute thunk and record its duration in a histogram instrument.
   Returns the thunk's result."
  (var start (clock/monotonic))
  (var result (thunk))
  (var elapsed (- (clock/monotonic) start))
  (telemetry-record histogram elapsed :attributes attributes)
  result)

# ============================================================================
# Exports
# ============================================================================

(fn []
  {# Meter lifecycle
   :meter     telemetry-meter
   :flush     telemetry-flush
   :shutdown  telemetry-shutdown

   # Instruments
   :counter   telemetry-counter
   :gauge     telemetry-gauge
   :histogram telemetry-histogram

   # Recording
   :add       telemetry-add
   :record    telemetry-record
   :set       telemetry-set

   # Convenience
   :time      telemetry-time

   # Low-level (for testing / custom export)
   :build-payload  build-export-payload
   :encode-attributes encode-attributes})
