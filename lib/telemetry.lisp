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
## Metric types: counter (monotonic and up-down), gauge, histogram.
## Export model: background fiber flushes at a configurable interval (default 60s).
## Manual flush via telemetry:flush.
##
## Data is pre-aggregated at record time (running sums, last-value gauges,
## histogram bucket counts).  Export snapshots and clears in one cooperative
## step — no races between background and manual flushes.

(def http ((import-file "lib/http.lisp")))

(def *telemetry-version* "0.2.0")

# ============================================================================
# Timestamp helpers
# ============================================================================

(defn now-nanos []
  "Current wall-clock time as integer nanoseconds (OTLP wire format)."
  (let [[secs (clock/realtime)]]
    (integer (* secs 1_000_000_000))))

# ============================================================================
# Attribute encoding (OTLP JSON wire format)
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
    (let [[result @[]]]
      (each [key val] in (pairs attrs)
        (push result {"key" (string key) "value" (encode-value val)}))
      (freeze result))))

# ============================================================================
# Attribute key — deterministic string for aggregation map lookups
# ============================================================================

(defn attrs-key [attrs]
  "Deterministic string key for an attribute set.
   Structs are BTreeMap-backed so (string (freeze ...)) is ordered."
  (if (or (nil? attrs) (empty? attrs))
    ""
    (string (freeze attrs))))

# ============================================================================
# Binary search for histogram bucket placement
# ============================================================================

(defn find-bucket [boundaries value]
  "Return index of first boundary >= value, or (length boundaries) for overflow.
   Boundaries must be sorted ascending."
  (var lo 0)
  (var hi (length boundaries))
  (while (< lo hi)
    (let [[mid (integer (/ (+ lo hi) 2))]]
      (if (<= value (get boundaries mid))
        (assign hi mid)
        (assign lo (+ mid 1)))))
  lo)

# ============================================================================
# Metric data structures
#
# Each metric is a mutable struct with pre-aggregated data in :aggregates.
# Counter  agg: @{:sum N :time nanos :attrs struct :count N}
# Gauge    agg: @{:value N :time nanos :attrs struct}
# Histogram agg: @{:counts @[] :sum F :count N :min N :max N :time nanos :attrs struct}
# ============================================================================

(defn make-counter [meter name unit description monotonic]
  @{:type        "sum"
    :name        name
    :unit        (or unit "1")
    :description (or description "")
    :meter       meter
    :monotonic   (if (nil? monotonic) true monotonic)
    :aggregates  @{}})

(defn make-gauge [meter name unit description]
  @{:type        "gauge"
    :name        name
    :unit        (or unit "1")
    :description (or description "")
    :meter       meter
    :aggregates  @{}})

(defn make-histogram [meter name unit description boundaries]
  @{:type        "histogram"
    :name        name
    :unit        (or unit "ms")
    :description (or description "")
    :meter       meter
    :boundaries  (or boundaries [0.005 0.01 0.025 0.05 0.075
                                  0.1 0.25 0.5 0.75 1.0 2.5
                                  5.0 7.5 10.0])
    :aggregates  @{}})

# ============================================================================
# Recording — pre-aggregate at record time
# ============================================================================

(defn add-point [metric value attrs]
  "Update running aggregate for a counter or gauge.
   Counters: running sum per attribute set.
   Gauges: last-value per attribute set (no growing list)."
  (block :done
    (when (not (get metric:meter :enabled)) (break :done nil))
    (let [[key   (attrs-key attrs)]
          [aggs  metric:aggregates]
          [now   (now-nanos)]]
      (if (= metric:type "gauge")
        # Gauge: replace with latest value
        (if (has? aggs key)
          (let [[agg (get aggs key)]]
            (put agg :value value)
            (put agg :time now))
          (put aggs key @{:value value :time now :attrs (or attrs {})}))
        # Counter: accumulate
        (if (has? aggs key)
          (let [[agg (get aggs key)]]
            (put agg :sum (+ agg:sum value))
            (put agg :time now)
            (put agg :count (+ agg:count 1)))
          (put aggs key @{:sum value :time now :attrs (or attrs {}) :count 1}))))))

(defn histogram-record [metric value attrs]
  "Update running histogram aggregate.
   Bucket placement uses binary search (O(log n) per observation)."
  (block :done
    (when (not (get metric:meter :enabled)) (break :done nil))
    (let* [[key    (attrs-key attrs)]
           [aggs   metric:aggregates]
           [now    (now-nanos)]
           [bounds metric:boundaries]
           [bi     (find-bucket bounds value)]]
      (if (has? aggs key)
        (let [[agg (get aggs key)]]
          (put (get agg :counts) bi (+ (get (get agg :counts) bi) 1))
          (put agg :sum (+ agg:sum (float value)))
          (put agg :count (+ agg:count 1))
          (put agg :min (min agg:min value))
          (put agg :max (max agg:max value))
          (put agg :time now))
        (begin
          (let [[counts @[]]]
            (var i 0)
            (while (<= i (length bounds))
              (push counts 0)
              (assign i (+ i 1)))
            (put counts bi 1)
            (put aggs key @{:counts counts
                            :sum    (float value)
                            :count  1
                            :min    value
                            :max    value
                            :time   now
                            :attrs  (or attrs {})})))))))

# ============================================================================
# Snapshot-and-clear — atomic under cooperative scheduling
#
# No yield points (no I/O) between reading and clearing aggregates,
# so background export and manual flush cannot race.
# ============================================================================

(defn snapshot-and-clear [meter]
  "Swap each metric's :aggregates with a fresh @{}.
   Returns a frozen list of {:metric m :snapshot aggs} pairs."
  (let [[snapshots @[]]]
    (each m in meter:metrics
      (let [[aggs m:aggregates]]
        (push snapshots {:metric m :snapshot aggs})
        (put m :aggregates @{})))
    # DELTA temporality: reset start time each export
    (when (= meter:temporality :delta)
      (put meter :start-nanos (now-nanos)))
    (freeze snapshots)))

# ============================================================================
# OTLP JSON payload construction (from pre-aggregated snapshots)
# ============================================================================

(defn encode-sum-datapoints [snapshot start-nanos]
  "Encode counter data points from pre-aggregated snapshot."
  (let [[result @[]]]
    (each [_ agg] in (pairs snapshot)
      (push result
        {"attributes"        (encode-attributes agg:attrs)
         "startTimeUnixNano" (string start-nanos)
         "timeUnixNano"      (string agg:time)
         "asDouble"          (float agg:sum)}))
    (freeze result)))

(defn encode-gauge-datapoints [snapshot]
  "Encode gauge data points — each entry is already the last value."
  (let [[result @[]]]
    (each [_ agg] in (pairs snapshot)
      (push result
        {"attributes"   (encode-attributes agg:attrs)
         "timeUnixNano" (string agg:time)
         "asDouble"     (float agg:value)}))
    (freeze result)))

(defn encode-histogram-datapoints [snapshot boundaries start-nanos]
  "Encode histogram data points from pre-computed bucket counts."
  (let [[result @[]]]
    (each [key agg] in (pairs snapshot)
      (push result
        {"attributes"        (encode-attributes agg:attrs)
         "startTimeUnixNano" (string start-nanos)
         "timeUnixNano"      (string agg:time)
         "count"             (string agg:count)
         "sum"               agg:sum
         "min"               (float agg:min)
         "max"               (float agg:max)
         "explicitBounds"    boundaries
         "bucketCounts"      (map (fn [c] (string c)) (freeze agg:counts))}))
    (freeze result)))

(defn temporality-code [meter]
  "OTLP aggregation temporality: 1=DELTA, 2=CUMULATIVE."
  (if (= meter:temporality :delta) 1 2))

(defn encode-metric [metric snapshot start-nanos]
  "Encode a single metric from its snapshot."
  (if (= (length (pairs snapshot)) 0)
    nil
    (let [[base {"name"        metric:name
                 "unit"        metric:unit
                 "description" metric:description}]
          [meter metric:meter]]
      (case metric:type
        "sum"
          (put base "sum"
            {"dataPoints"             (encode-sum-datapoints snapshot start-nanos)
             "aggregationTemporality" (temporality-code meter)
             "isMonotonic"            metric:monotonic})

        "gauge"
          (put base "gauge"
            {"dataPoints" (encode-gauge-datapoints snapshot)})

        "histogram"
          (put base "histogram"
            {"dataPoints"             (encode-histogram-datapoints
                                        snapshot metric:boundaries start-nanos)
             "aggregationTemporality" (temporality-code meter)})
        base))))

(defn build-export-payload [meter snapshots]
  "Build the full OTLP ExportMetricsServiceRequest JSON body from snapshots."
  (let [[encoded @[]]]
    (each snap in snapshots
      (let [[enc (encode-metric (get snap :metric) (get snap :snapshot) meter:start-nanos)]]
        (unless (nil? enc)
          (push encoded enc))))
    (if (empty? encoded)
      nil
      {"resourceMetrics"
       [{"resource"
         {"attributes" (encode-attributes meter:resource)}
         "scopeMetrics"
         [{"scope" {"name" "elle-telemetry" "version" *telemetry-version*}
           "metrics" (freeze encoded)}]}]})))

(defn build-payload-peek [meter]
  "Build payload from current live aggregates without clearing.
   For inspection and testing — does not affect export state."
  (let [[snapshots @[]]]
    (each m in meter:metrics
      (push snapshots {:metric m :snapshot m:aggregates}))
    (build-export-payload meter (freeze snapshots))))

# ============================================================================
# Export with retry and data recovery
# ============================================================================

(defn merge-snapshot-back [snapshots]
  "Merge snapshot aggregates back into live metrics after failed export.
   Counters/histograms: add values back.  Gauges: skip (live is newer)."
  (each snap in snapshots
    (let [[metric (get snap :metric)]
          [old    (get snap :snapshot)]]
      (each [key agg] in (pairs old)
        (let [[live metric:aggregates]]
          (if (has? live key)
            (case metric:type
              "sum"
                (let [[l (get live key)]]
                  (put l :sum (+ l:sum agg:sum))
                  (put l :count (+ l:count agg:count)))
              "gauge" nil   # live gauge is newer — don't overwrite
              "histogram"
                (let [[l (get live key)]]
                  (put l :sum (+ l:sum agg:sum))
                  (put l :count (+ l:count agg:count))
                  (put l :min (min l:min agg:min))
                  (put l :max (max l:max agg:max))
                  (var i 0)
                  (while (< i (length l:counts))
                    (put l:counts i (+ (get l:counts i) (get agg:counts i)))
                    (assign i (+ i 1))))
              nil)
            # No live entry yet — restore the snapshot entry
            (put live key agg)))))))

(defn do-export [meter]
  "Export: snapshot-and-clear, build payload, HTTP POST with retry.
   On final failure, merge snapshot back so data is not lost."
  (let [[snapshots (snapshot-and-clear meter)]]
    (let [[payload (build-export-payload meter snapshots)]]
      (if (nil? payload)
        nil
        (let [[body     (json-serialize payload)]
              [endpoint meter:endpoint]
              [headers  (or meter:headers {})]]
          # On-export hook
          (when meter:on-export
            (meter:on-export payload))
          # Export with retry (up to 3 attempts, backoff 1s/2s)
          (var attempts 0)
          (var success false)
          (while (and (< attempts 3) (not success))
            (let [[[ok? result] (protect
                    (http:post endpoint body
                      :headers (merge {:content-type "application/json"} headers)))]]
              (if (and ok? (>= result:status 200) (< result:status 300))
                (assign success true)
                (begin
                  (assign attempts (+ attempts 1))
                  (when (< attempts 3)
                    (ev/sleep attempts))))))
          (unless success
            (eprintln "telemetry: export failed after 3 attempts, recovering data")
            (merge-snapshot-back snapshots))
          success)))))

# ============================================================================
# Background export fiber
# ============================================================================

(defn start-export-loop [meter]
  "Spawn a background fiber that exports at the configured interval."
  (ev/spawn (fn []
    (let [[interval meter:interval]]
      (forever
        (ev/sleep interval)
        (let [[[ok? err] (protect (do-export meter))]]
          (unless ok?
            (eprintln "telemetry: background export error")))
        (when meter:shutdown?
          (break)))))))

# ============================================================================
# Meter (top-level registry)
# ============================================================================

(defn telemetry-meter [service-name &named endpoint interval resource headers
                                          temporality enabled on-export]
  "Create a meter (metric registry + exporter).
   :endpoint    — OTLP/HTTP endpoint URL (default: Prometheus OTLP receiver)
   :interval    — export interval in seconds (default: 60)
   :resource    — extra resource attributes struct
   :headers     — extra HTTP headers for export requests
   :temporality — :cumulative (default) or :delta
   :enabled     — false to disable recording (default: true)
   :on-export   — (fn [payload]) callback before HTTP send
   Returns a mutable struct."
  (let* [[sdk-attrs {"telemetry.sdk.name"     "elle-telemetry"
                     "telemetry.sdk.version"   *telemetry-version*
                     "telemetry.sdk.language"  "elle"}]
          [base-resource (merge (merge sdk-attrs
                                       {"service.name" service-name})
                                (or resource {}))]]
    (def meter
      @{:service     service-name
        :endpoint    (or endpoint "http://localhost:9090/api/v1/otlp/v1/metrics")
        :interval    (or interval 60)
        :resource    base-resource
        :headers     (or headers {})
        :metrics     @[]
        :start-nanos (now-nanos)
        :shutdown?   false
        :exporter    nil
        :temporality (or temporality :cumulative)
        :enabled     (if (nil? enabled) true enabled)
        :on-export   on-export})
    (put meter :exporter (start-export-loop meter))
    meter))

# ============================================================================
# Instrument constructors
# ============================================================================

(defn telemetry-counter [meter name &named unit description monotonic]
  "Create a counter. Monotonic by default; pass :monotonic false for UpDownCounter."
  (let [[c (make-counter meter name unit description monotonic)]]
    (push meter:metrics c)
    c))

(defn telemetry-gauge [meter name &named unit description]
  "Create a gauge (last-value instrument)."
  (let [[g (make-gauge meter name unit description)]]
    (push meter:metrics g)
    g))

(defn telemetry-histogram [meter name &named unit description boundaries]
  "Create a histogram with explicit bucket boundaries.
   Default boundaries are HTTP latency buckets (seconds)."
  (let [[h (make-histogram meter name unit description boundaries)]]
    (push meter:metrics h)
    h))

# ============================================================================
# Recording API
# ============================================================================

(defn telemetry-add [instrument value &named attributes]
  "Add a value to a counter or gauge."
  (add-point instrument value attributes))

(defn telemetry-record [instrument value &named attributes]
  "Record an observation to a histogram."
  (histogram-record instrument value attributes))

(defn telemetry-observe [instrument value &named attributes]
  "Record an observation to a histogram (alias for :record)."
  (histogram-record instrument value attributes))

(defn telemetry-set [instrument value &named attributes]
  "Set a gauge to a specific value."
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
  (put meter :shutdown? true)
  (let [[exporter meter:exporter]]
    (when exporter
      (ev/abort exporter)))
  nil)

(defn telemetry-enabled? [meter]
  "Check if the meter is enabled for recording."
  meter:enabled)

# ============================================================================
# Convenience: timed operation
# ============================================================================

(defn telemetry-time [histogram thunk &named attributes]
  "Execute thunk and record its duration in a histogram instrument.
   Returns the thunk's result."
  (let* [[start (clock/monotonic)]
         [result (thunk)]
         [elapsed (- (clock/monotonic) start)]]
    (telemetry-record histogram elapsed :attributes attributes)
    result))

# ============================================================================
# Exports
# ============================================================================

(fn []
  {# Meter lifecycle
   :meter       telemetry-meter
   :flush       telemetry-flush
   :shutdown    telemetry-shutdown
   :enabled?    telemetry-enabled?

   # Instruments
   :counter     telemetry-counter
   :gauge       telemetry-gauge
   :histogram   telemetry-histogram

   # Recording
   :add         telemetry-add
   :record      telemetry-record
   :observe     telemetry-observe
   :set         telemetry-set

   # Convenience
   :time        telemetry-time

   # Low-level (for testing / custom export)
   :build-payload  build-payload-peek
   :encode-attributes encode-attributes})
