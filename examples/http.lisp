#!/usr/bin/env elle

# HTTP — server and client in a single event loop
#
# Demonstrates:
#   http:serve                        — accept loop with fiber-per-connection
#   http:connect/send/close           — keep-alive client session
#   http:get                          — one-shot client request
#   http:respond                      — response construction
#   http:parse-url                    — URL parsing
#   ev/run + ev/spawn                 — single async event loop
#
# The server accepts connections and routes requests.  The client exercises
# keep-alive (two requests on one TCP connection) then one-shot mode.

(def http ((import-file "./lib/http.lisp")))


# ── URL parsing (pure, no I/O) ──────────────────────────────────────────────

(let [[u (http:parse-url "http://example.com:8080/api?q=test")]]
  (assert (= u:host "example.com") "parse-url: host")
  (assert (= u:port 8080)          "parse-url: port")
  (assert (= u:path "/api")        "parse-url: path")
  (assert (= u:query "q=test")     "parse-url: query"))

(let [[u (http:parse-url "http://localhost/")]]
  (assert (= u:port 80)  "parse-url: default port")
  (assert (= u:path "/") "parse-url: root path"))


# ── Response construction (pure, no I/O) ────────────────────────────────────

(let [[r (http:respond 200 "hello")]]
  (assert (= r:status 200)                              "respond: status")
  (assert (= r:body "hello")                            "respond: body")
  (assert (= (get r:headers :content-length) "5")       "respond: content-length")
  (assert (= (get r:headers :content-type) "text/plain") "respond: content-type"))


# ── Server + client integration ─────────────────────────────────────────────
#
# Architecture:
#   1. Bind a TCP listener on port 0 (OS-assigned)
#   2. ev/run launches two fibers:
#      - Server: http:serve handles accept loop + connection handling
#      - Client: exercises keep-alive then one-shot modes
#   3. Client calls ev/shutdown when done, which aborts the server fiber
#      and gives it a chance to clean up (close listener, connections).

(var request-count 0)

(defn handler [request]
  "Route requests and count them."
  (assign request-count (+ request-count 1))
  (cond
    ((= request:path "/hello")
     (http:respond 200 "Hello, World!"))

    ((= request:path "/echo")
     (http:respond 201 request:body
       :headers {:content-type "application/octet-stream"}))

    ((= request:path "/count")
     (http:respond 200 (string request-count)))

    (true
     (http:respond 404 "not found"))))

(let [[listener (tcp/listen "127.0.0.1" 0)]]
  (let* [[addr (port/path listener)]
         [port-num (integer (get (string/split addr ":") 1))]]
    (print "  server listening on port ") (println port-num)
    (let [[results @[]]]
      (ev/run
        # Server fiber
        (fn [] (http:serve listener handler))

        # Client fiber
        (fn []

          # ── Keep-alive: two requests on one TCP connection ──

          (let [[session (http:connect
                           (string/format "http://127.0.0.1:{}/" port-num))]]
            (let [[r1 (http:send session "GET" "/hello")]]
              (print "  keep-alive GET /hello: ") (println r1:status)
              (push results r1))

            (let [[r2 (http:send session "POST" "/echo"
                        :body "ping" :headers {:content-type "text/plain"})]]
              (print "  keep-alive POST /echo: ") (println r2:status)
              (push results r2))

            (http:close session))

          # ── One-shot: new TCP connection, connection: close ──

          (let [[r3 (http:get
                       (string/format "http://127.0.0.1:{}/count" port-num))]]
            (print "  one-shot GET /count: ") (println r3:body)
            (push results r3))

          # ── Shut down the event loop ─────────────────────────
          # Aborts the server fiber (cancels pending accept I/O),
          # lets defer blocks run, then exits ev/run.
          (ev/shutdown 100)))

      # ── Assertions ──────────────────────────────────────────────

      (let [[[r1 r2 r3] results]]

        # Keep-alive GET
        (assert (= r1:status 200)            "keep-alive GET: status 200")
        (assert (= r1:body "Hello, World!")  "keep-alive GET: body")

        # Keep-alive POST
        (assert (= r2:status 201)            "keep-alive POST: status 201")
        (assert (= r2:body "ping")           "keep-alive POST: echoed body")
        (assert (= (get r2:headers :content-type) "application/octet-stream")
          "keep-alive POST: custom content-type")

        # One-shot GET (connection: close)
        (assert (= r3:status 200)            "one-shot GET: status 200")
        (assert (= r3:body "3")              "one-shot GET: 3 requests served"))

      (assert (= request-count 3) "server handled exactly 3 requests"))))

(println "")
(println "all http passed.")
