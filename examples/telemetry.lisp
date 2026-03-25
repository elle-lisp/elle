(elle/epoch 6)
#!/usr/bin/env elle

# A small bookstore API that tracks its own metrics.
#
# The store handles orders, searches, and inventory checks.  Every request
# is counted, timed, and — for purchases — the revenue is accumulated.
# A gauge tracks the number of books currently in stock.
#
# Metrics are exported to Prometheus via OTLP/HTTP every 10 seconds.
# Open http://localhost:9090 and try:
#
#   bookstore_orders_total
#   rate(bookstore_request_duration_seconds_sum[1m])
#   bookstore_inventory_books
#
# Run:  elle examples/telemetry.lisp

(def telemetry ((import "telemetry")))

# ── The meter ────────────────────────────────────────────────────────

(def meter (telemetry:meter "bookstore"
  :endpoint "http://localhost:9090/api/v1/otlp/v1/metrics"
  :interval 10
  :resource {"deployment.environment" "dev"
             "host.name"              "laptop"}))

# ── Instruments ──────────────────────────────────────────────────────

(def requests
  (telemetry:counter meter "bookstore_requests"
    :unit "1" :description "Total API requests"))

(def latency
  (telemetry:histogram meter "bookstore_request_duration"
    :unit "s" :description "Request processing time"
    :boundaries [0.001 0.005 0.01 0.025 0.05 0.1 0.25 0.5 1.0]))

(def orders
  (telemetry:counter meter "bookstore_orders"
    :unit "1" :description "Completed orders"))

(def revenue
  (telemetry:counter meter "bookstore_revenue"
    :unit "USD" :description "Total revenue"))

(def inventory
  (telemetry:gauge meter "bookstore_inventory_books"
    :unit "1" :description "Books currently in stock"))

# ── Simulated catalog ───────────────────────────────────────────────

(def catalog
  {"978-0-13-468599-1" {:title "The C Programming Language"   :price 45.00  :stock 12}
   "978-0-262-51087-5" {:title "SICP"                         :price 55.00  :stock 8}
   "978-0-321-12521-7" {:title "Domain-Driven Design"         :price 52.00  :stock 5}
   "978-1-49-195016-0" {:title "Designing Data-Intensive Apps" :price 48.00  :stock 15}
   "978-0-596-51774-8" {:title "JavaScript: The Good Parts"   :price 25.00  :stock 20}})

(var stock @{})
(each [isbn book] in (pairs catalog)
  (put stock isbn book:stock))

(defn books-in-stock []
  (fold (fn [acc [_ n]] (+ acc n)) 0 (pairs stock)))

# ── Request handlers ────────────────────────────────────────────────

(defn handle-search [query]
  "Search the catalog by title substring."
  (var results @[])
  (each [isbn book] in (pairs catalog)
    (when (string-contains? (string/lowercase book:title)
                            (string/lowercase query))
      (push results isbn)))
  (freeze results))

(defn handle-purchase [isbn qty]
  "Buy qty copies.  Returns {:ok true :total N} or {:ok false :reason ...}."
  (let [[avail (get stock isbn)]]
    (cond
      ((nil? avail) {:ok false :reason "not found"})
      ((> qty avail) {:ok false :reason "insufficient stock"})
      (true
        (let* [[price (get (get catalog isbn) :price)]
               [total (* price qty)]]
          (put stock isbn (- avail qty))
          (telemetry:add orders 1
            :attributes {"isbn" isbn})
          (telemetry:add revenue total
            :attributes {"currency" "USD"})
          {:ok true :total total})))))

(defn handle-request [method path]
  "Dispatch a simulated API request."
  (var attrs {"method" method "path" path})
  (telemetry:add requests 1 :attributes attrs)
  (telemetry:time latency
    (fn []
      (case path
        "/search"    (begin (ev/sleep (/ (+ 1 (mod (length method) 5)) 1000.0))
                            (handle-search "programming"))
        "/purchase"  (begin (ev/sleep (/ (+ 2 (mod (length path) 8)) 1000.0))
                            (handle-purchase "978-0-262-51087-5" 1))
        "/inventory" (begin (ev/sleep 0.001)
                            (books-in-stock))
        (begin (ev/sleep 0.001) nil)))
    :attributes attrs))

# ── Run the "app" ───────────────────────────────────────────────────

(println "bookstore starting (Ctrl-C to stop)")
(println "  metrics → http://localhost:9090")
(println "")

# Set initial inventory gauge
(telemetry:set inventory (books-in-stock)
  :attributes {"warehouse" "main"})

# Simulate a burst of traffic
(defn traffic-burst [label reqs]
  (println "  " label "...")
  (each [method path] in reqs
    (handle-request method path))
  # Update inventory after purchases
  (telemetry:set inventory (books-in-stock)
    :attributes {"warehouse" "main"}))

(traffic-burst "morning rush"
  [["GET"  "/search"]
   ["GET"  "/search"]
   ["POST" "/purchase"]
   ["GET"  "/inventory"]
   ["POST" "/purchase"]
   ["GET"  "/search"]
   ["POST" "/purchase"]
   ["GET"  "/inventory"]])

(traffic-burst "lunch lull"
  [["GET"  "/search"]
   ["GET"  "/inventory"]])

(traffic-burst "afternoon spike"
  [["POST" "/purchase"]
   ["GET"  "/search"]
   ["POST" "/purchase"]
   ["POST" "/purchase"]
   ["GET"  "/search"]
   ["GET"  "/search"]
   ["GET"  "/inventory"]
   ["POST" "/purchase"]
   ["GET"  "/search"]
   ["GET"  "/404"]])

# Flush to Prometheus
(println "")
(println "  flushing to Prometheus...")
(telemetry:flush meter)
(println "  done")
(println "")

(println "  check Prometheus at http://localhost:9090:")
(println "    bookstore_requests_total")
(println "    bookstore_orders_total")
(println "    bookstore_revenue_USD_total")
(println "    bookstore_inventory_books")
(println "    histogram_quantile(0.95, rate(bookstore_request_duration_seconds_bucket[5m]))")

(telemetry:shutdown meter)
(println "")
(println "bookstore stopped.")
