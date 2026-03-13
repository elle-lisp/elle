#!/usr/bin/env elle

## HTTP module example
##
## Demonstrates:
##   parse-url    — URL parsing
##   http:respond — build response structs
##   http:get     — make a GET request
##   http:post    — make a POST request with a body
##   Integration  — live server+client round-trip using ev/run

(def http ((import-file "./lib/http.lisp")))

(def parse-url      (get http :parse-url))
(def http-get       (get http :http-get))
(def http-post      (get http :http-post))
(def http-respond   (get http :http-respond))
(def read-request   (get http :read-request))
(def write-response (get http :write-response))

# ── URL parsing ──────────────────────────────────────────────────────────────

(let* [[u (parse-url "http://example.com:8080/api?q=test")]]
  (assert (= u:host  "example.com") "host")
  (assert (= u:port  8080)          "port")
  (assert (= u:path  "/api")        "path")
  (assert (= u:query "q=test")      "query"))

# ── http-respond ─────────────────────────────────────────────────────────────

(let* [[r (http-respond 200 "Hello, World!")]]
  (assert (= r:status 200)             "status 200")
  (assert (= r:body   "Hello, World!") "body"))

# ── Server + client integration ──────────────────────────────────────────────
#
# We create a TCP listener ourselves so we can control the lifecycle.
# The server fiber handles exactly 2 connections then exits.
# The client fiber makes 2 requests then exits.
# ev/run returns once both fibers are done.

(let* [[listener (tcp/listen "127.0.0.1" 0)]
       [addr     (port/path listener)]
       [port-num (integer (get (string/split addr ":") 1))]
       [responses @[]]]

  (ev/run
    # Server: handle exactly 2 connections
    (fn []
      (let* [[conn (tcp/accept listener)]
             [req  (read-request conn)]
             [body (string/format "method={} path={}" req:method req:path)]]
        (write-response conn (http-respond 200 body))
        (port/close conn))
      (let* [[conn (tcp/accept listener)]
             [req  (read-request conn)]
             [body (string/format "body={}" (or req:body ""))]]
        (write-response conn (http-respond 201 body))
        (port/close conn)))

    # Client: GET then POST, collect responses
    (fn []
      (push responses (http-get  (string/format "http://127.0.0.1:{}/hello" port-num)))
      (push responses (http-post (string/format "http://127.0.0.1:{}/data"  port-num) "hello"))))

  (port/close listener)

  (let* [[r1 (get responses 0)]
         [r2 (get responses 1)]]
    (assert (= r1:status 200)                  "GET status 200")
    (assert (string-contains? r1:body "GET")   "GET response mentions method")
    (assert (= r2:status 201)                  "POST status 201")
    (assert (string-contains? r2:body "hello") "POST response echoes body")))

(print "all http examples passed.")
