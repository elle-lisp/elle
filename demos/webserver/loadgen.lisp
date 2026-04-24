#!/usr/bin/env elle
(elle/epoch 9)

# Concurrent HTTP load generator with latency stats
#
# Usage:
#   elle demos/webserver/loadgen.lisp [url] [requests] [concurrency] [keepalive]
#
# Defaults:
#   url         http://127.0.0.1:8080/
#   requests    1000
#   concurrency 50
#
# The optional fourth argument "keepalive" reuses persistent connections
# (one per worker). Without it, each request opens a fresh connection —
# the realistic baseline for independent clients.

(def http ((import "std/http")))

# ── Parameters ───────────────────────────────────────────────────────

(def args      (sys/args))
(def target    (or (get args 0) "http://127.0.0.1:8080/"))
(def total     (parse-int (or (get args 1) "1000")))
(def parallel  (parse-int (or (get args 2) "50")))
(def keepalive (= (get args 3) "keepalive"))

(def parsed (http:parse-url target))
(def path   (or parsed:path "/"))

# ── Single request (fresh connection) ────────────────────────────────

(defn fresh-request [_]
  (let* [t0         (clock/monotonic)
         [ok? resp] (protect (http:get target))
         t1         (clock/monotonic)]
    {:ok?        ok?
     :status     (if ok? resp:status 0)
     :latency-ms (* (- t1 t0) 1000.0)}))

# ── Keep-alive worker (one connection, N requests) ───────────────────

(defn distribute [total n]
  "Split total into n roughly-equal chunks."
  (let* [base   (/ total n)
         extra  (% total n)
         result @[]]
    (def @i 0)
    (while (< i n)
      (push result (+ base (if (< i extra) 1 0)))
      (assign i (+ i 1)))
    (freeze result)))

(defn keepalive-worker [request-count]
  (let* [session (http:connect target)
         results @[]]
    (defer (protect (http:close session))
      (def @i 0)
      (while (< i request-count)
        (let* [t0         (clock/monotonic)
               [ok? resp] (protect (http:send session "GET" path))
               t1         (clock/monotonic)]
          (push results {:ok?        ok?
                         :status     (if ok? resp:status 0)
                         :latency-ms (* (- t1 t0) 1000.0)}))
        (assign i (+ i 1))))
    (->list results)))

# ── Percentile helper ────────────────────────────────────────────────

(defn percentile [sorted-arr p]
  (let* [n   (length sorted-arr)
         idx (min (- n 1) (floor (* (/ p 100.0) n)))]
    (get sorted-arr idx)))

# ── Stats printer ────────────────────────────────────────────────────

(defn print-stats [results elapsed]
  (let* [n           (length results)
         rps         (/ n elapsed)
         latencies   (sort (map (fn [r] r:latency-ms) results))
         lat-arr     (->array latencies)
         ok-count    (length (filter (fn [r] r:ok?) results))
         err-count   (- n ok-count)
         status-dist (fold (fn [acc r]
                             (let [k (string r:status)]
                               (put acc k (+ (or (get acc k) 0) 1))))
                           {} results)]
    (println "")
    (println "── results ────────────────────────────────────────")
    (println (string/format "total requests:  {}" n))
    (println (string/format "elapsed:         {:.2f}s" elapsed))
    (println (string/format "requests/sec:    {:.1f}" rps))
    (println (string/format "ok / errors:     {} / {}" ok-count err-count))
    (println "")
    (println "── latency (ms) ──────────────────────────────────")
    (println (string/format "  min:  {:.2f}" (get lat-arr 0)))
    (println (string/format "  p50:  {:.2f}" (percentile lat-arr 50)))
    (println (string/format "  p95:  {:.2f}" (percentile lat-arr 95)))
    (println (string/format "  p99:  {:.2f}" (percentile lat-arr 99)))
    (println (string/format "  max:  {:.2f}" (get lat-arr (- (length lat-arr) 1))))
    (println "")
    (println "── status codes ──────────────────────────────────")
    (each k in (keys status-dist)
      (println (string/format "  {}: {}" k (get status-dist k))))))

# ── Main ─────────────────────────────────────────────────────────────

(def mode (if keepalive "keepalive" "fresh"))
(println (string/format "load test: {} requests, {} concurrent, {} connections → {}"
                        total parallel mode target))

(let* [t0      (clock/monotonic)
       results (if keepalive
                 (let* [chunks  (distribute total parallel)
                        batches (ev/map-limited keepalive-worker chunks parallel)]
                   (->array (fold concat () batches)))
                 (ev/map-limited fresh-request (range total) parallel))
       elapsed (- (clock/monotonic) t0)]
  (print-stats results elapsed))
