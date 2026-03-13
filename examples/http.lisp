#!/usr/bin/env elle

## HTTP module example
##
## Demonstrates:
##   http:serve   — start a server on a random port
##   http:get     — make a GET request
##   http:post    — make a POST request with a body
##   http:respond — build response structs
##   parse-url    — URL parsing

(def http ((import-file "./lib/http.lisp")))

(def parse-url   (get http :parse-url))
(def http-get    (get http :http-get))
(def http-post   (get http :http-post))
(def http-respond (get http :http-respond))

# ── URL parsing ──────────────────────────────────────────────────────────────

(let [[u (parse-url "http://example.com:8080/api?q=test")]]
  (assert (= u:host  "example.com") "host")
  (assert (= u:port  8080)          "port")
  (assert (= u:path  "/api")        "path")
  (assert (= u:query "q=test")      "query"))

# ── http:respond ─────────────────────────────────────────────────────────────

(let [[r (http-respond 200 "Hello, World!")]]
  (assert (= r:status 200)           "status 200")
  (assert (= r:body   "Hello, World!") "body"))

# ── Server + client integration ──────────────────────────────────────────────

(let* [[listener (tcp/listen "127.0.0.1" 0)]
       [addr     (port/path listener)]
       [port-num (integer (get (string/split addr ":") 1))]]
  (let [[responses @[]]]
    (ev/run
      # Server: handle exactly 2 connections then stop
      (fn ()
        (let [[conn (tcp/accept listener)]]
          (defer (port/close conn)
            (let [[req ((get http :http-serve) conn)]]
              ((get http :write-response) conn
                (http-respond 200 (string/format "method={} path={}" req:method req:path))))))
        (let [[conn (tcp/accept listener)]]
          (defer (port/close conn)
            (let [[req ((get http :http-serve) conn)]]
              ((get http :write-response) conn
                (http-respond 201 (string/format "body={}" req:body)))))))

      # Client: GET then POST
      (fn ()
        (push responses (http-get  (string/format "http://127.0.0.1:{}/hello" port-num)))
        (push responses (http-post (string/format "http://127.0.0.1:{}/data"  port-num) "hello"))))

    (port/close listener)

    (let [[r1 (get responses 0)]
          [r2 (get responses 1)]]
      (assert (= r1:status 200)                    "GET status 200")
      (assert (string-contains? r1:body "GET")     "GET response mentions method")
      (assert (= r2:status 201)                    "POST status 201")
      (assert (string-contains? r2:body "hello")   "POST response echoes body")))

  (print "all http examples passed."))
