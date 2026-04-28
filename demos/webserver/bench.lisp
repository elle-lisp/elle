#!/usr/bin/env elle
(elle/epoch 9)

# Webserver performance profiling — sweeps concurrency and request
# count, charts latency distribution and throughput via plotters.
#
# Usage:
#   # terminal 1: start server
#   elle demos/webserver/server.lisp 8080
#
#   # terminal 2: run bench + graph
#   elle demos/webserver/bench.lisp [base-url]

(def http ((import "std/http")))
(def plt (import "plugin/plotters"))

(def base-url (or (get (sys/args) 0) "http://127.0.0.1:8080/"))
(def parsed (http:parse-url base-url))
(def path (or parsed:path "/"))
(def svg-opts {:format :svg})

# ── Single request ───────────────────────────────────────────────────

(defn timed-request [_]
  (let* [t0 (clock/monotonic)
         [ok? resp] (protect (http:get base-url))
         t1 (clock/monotonic)]
    {:ok? ok? :status (if ok? resp:status 0) :latency-ms (* (- t1 t0) 1000.0)}))

# ── Run one trial ────────────────────────────────────────────────────

(defn trial [total concurrency]
  (let* [t0 (clock/monotonic)
         results (ev/map-limited timed-request (range total) concurrency)
         elapsed (- (clock/monotonic) t0)
         lats (sort (map (fn [r] r:latency-ms) results))
         lat-arr (->array lats)
         n (length lat-arr)
         ok (length (filter (fn [r] r:ok?) results))]
    {:concurrency concurrency
     :total total
     :elapsed elapsed
     :rps (/ n elapsed)
     :ok ok
     :errors (- n ok)
     :min (get lat-arr 0)
     :p50 (get lat-arr (floor (* 0.5 n)))
     :p95 (get lat-arr (min (- n 1) (floor (* 0.95 n))))
     :p99 (get lat-arr (min (- n 1) (floor (* 0.99 n))))
     :max (get lat-arr (- n 1))
     :latencies lat-arr}))

# ── Concurrency sweep ────────────────────────────────────────────────

(def concurrency-levels [1 5 10 25 50 100])
(def requests-per-level 500)

(println (string/format "benchmarking {} → {} requests per level" base-url
           requests-per-level))
(println "")

(def @results @[])
(each c in concurrency-levels
  (let [r (trial requests-per-level c)]
    (push results r)
    (println (string/format (string "  c={:>3}  rps={:.0f}"
                 "  p50={:.1f}ms  p95={:.1f}ms" "  p99={:.1f}ms  errors={}") c
               r:rps r:p50 r:p95 r:p99 r:errors))))

# ── Chart 1: Throughput vs concurrency (line) ────────────────────────

(def rps-data (->array (map (fn [r] [(float r:concurrency) r:rps]) results)))
(spit "demos/webserver/throughput.svg"
  (plt:line rps-data
    (merge svg-opts
      {:title "throughput vs concurrency"
       :x-label "concurrent connections"
       :y-label "requests/sec"
       :width 900
       :height 500})))
(println "")
(println "wrote demos/webserver/throughput.svg")

# ── Chart 2: Latency percentiles vs concurrency (multi-series) ──────

(def p50-data (->array (map (fn [r] [(float r:concurrency) r:p50]) results)))
(def p95-data (->array (map (fn [r] [(float r:concurrency) r:p95]) results)))
(def p99-data (->array (map (fn [r] [(float r:concurrency) r:p99]) results)))

(spit "demos/webserver/latency.svg"
  (plt:chart (merge svg-opts
               {:title "latency vs concurrency"
                :x-label "concurrent connections"
                :y-label "latency (ms)"
                :width 900
                :height 500
                :series [{:type :line :label "p50" :data p50-data :color :blue}
                         {:type :line :label "p95" :data p95-data :color :orange}
                         {:type :line :label "p99" :data p99-data :color :red}]})))
(println "wrote demos/webserver/latency.svg")

# ── Chart 3: Latency histogram at peak concurrency ───────────────────

(def peak (get results (- (length results) 1)))
(spit "demos/webserver/histogram.svg"
  (plt:histogram peak:latencies
    (merge svg-opts
      {:title (string/format "latency distribution (c={})" peak:concurrency)
       :x-label "latency (ms)"
       :y-label "count"
       :bins 30
       :width 900
       :height 500})))
(println "wrote demos/webserver/histogram.svg")

(println "")
(println "done — view with: imv demos/webserver/*.svg")
