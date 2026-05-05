#!/usr/bin/env elle
(elle/epoch 10)

# Demo HTTP server
#
# Endpoints:
#   GET  /           welcome page (plain text)
#   GET  /health     {"status":"ok"} JSON
#   POST /echo       echo request body back
#   GET  /delay/:ms  sleep then respond (latency testing)
#   GET  /counter    return and increment a hit counter
#   GET  /stats      JSON with uptime and request count
#   *    *           404
#
# Usage:
#   elle demos/webserver/server.lisp [port]

(def http ((import "std/http")))

# ── Mutable state ────────────────────────────────────────────────────

(def @request-count 0)
(def start-time (clock/monotonic))

# ── Response helpers ─────────────────────────────────────────────────

(defn json-response [status body]
  "Build a JSON response with correct content-type."
  (let [json (json/serialize body)]
    {:status status
     :headers {:content-type "application/json"
               :content-length (string (string/size-of json))}
     :body json}))

# ── Request handler ──────────────────────────────────────────────────

(defn handler [req]
  (assign request-count (+ request-count 1))
  (let [path req:path
        method req:method]
    (cond
      (= path "/") (http:respond 200 "welcome to the elle demo server")

      (= path "/health") (json-response 200 {:status "ok"})

      (and (= method "POST") (= path "/echo")) (http:respond 200
      (or req:body ""))

      (string/starts-with? path "/delay/")
        (let* [parts (string/split path "/")
               ms (parse-int (get parts 2))]
          (ev/sleep (/ ms 1000.0))
          (http:respond 200 (string/format "delayed {}ms" ms)))
      (= path "/counter") (http:respond 200 (string request-count))

      (= path "/stats")
        (let [uptime (- (clock/monotonic) start-time)]
          (json-response 200 {:uptime-s uptime :requests request-count}))
      true (http:respond 404 "not found"))))

# ── Main ─────────────────────────────────────────────────────────────

(def port (parse-int (or (get (sys/args) 0) "8080")))
(def listener (tcp/listen "0.0.0.0" port))
(println (string/format "listening on 0.0.0.0:{}" port))
(http:serve listener handler)
