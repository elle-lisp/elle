(elle/epoch 9)
## tests/h2-same-scheduler.lisp — h2 client+server in the same scheduler
##
## Validates that http2:serve and http2:connect work when both client
## and server fibers share a single scheduler (the common case for
## proxy, test, and loopback scenarios).
##
## See tests/h2-server.lisp for comprehensive server coverage.
##
## Regression test for two bugs in server-connection's handler fibers:
##   1. stream:transition :send-headers is invalid from :open or
##      :half-closed-remote (reader already transitioned past :idle)
##   2. Nested if/apply expressions inside struct literals trigger
##      "expected hashable value, got struct" in fiber contexts

(def http2 ((import "std/http2")))

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn with-server [handler test-fn]
  "Start an h2-serve listener, run test-fn with [session lport], clean up."
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [[ok? _] (protect (http2:serve listener handler))]
                          nil)))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer (begin
             (protect (http2:close session))
             (protect (port/close listener))
             (protect (ev/abort sf)))
           (test-fn session))))

## ── Test 1: single request ─────────────────────────────────────────────

(defn test-single-request []
  (with-server (fn [req] {:status 200 :body (concat "echo:" req:path)})
               (fn [session]
                 (let [resp (http2:send session "GET" "/hello")]
                   (assert (= resp:status 200)
                           (concat "status should be 200, got "
                                   (string resp:status)))
                   (assert (= (string resp:body) "echo:/hello")
                           (concat "body should be echo:/hello, got "
                                   (string resp:body))))))
  (println "  PASS: single request"))

## ── Test 2: multiple sequential requests ───────────────────────────────

(defn test-sequential-requests []
  (with-server (fn [req] {:status 200 :body (concat "seq:" req:path)})
               (fn [session]
                 (each i in (range 0 10)
                   (let [resp (http2:send session
                         "GET"
                         (concat "/req-" (string i)))]
                     (assert (= resp:status 200)
                             (concat "seq req " (string i) " status"))
                     (assert (= (string resp:body)
                                (concat "seq:/req-" (string i)))
                             (concat "seq req " (string i) " body"))))))
  (println "  PASS: 10 sequential requests"))

## ── Test 3: request with body ──────────────────────────────────────────

(defn test-request-with-body []
  (with-server (fn [req]
                 {:status 200
                  :body (if (nil? req:body) "nobody" (string req:body))})
               (fn [session]
                 (let [resp (http2:send session
                                        "POST"
                                        "/data"
                                        :body "hello world")]
                   (assert (= resp:status 200)
                           (concat "post: status " (string resp:status)))
                   (assert (= (string resp:body) "hello world")
                           (concat "post: body " (string resp:body))))))
  (println "  PASS: request with body"))

## ── Test 4: response with trailers ───────────────────────────────────

(defn test-trailers-with-body []
  (with-server (fn [req]
                 {:status 200
                  :headers {:content-type "application/grpc"}
                  :body "payload"
                  :trailers [["grpc-status" "0"] ["custom-trailer" "value"]]})
               (fn [session]
                 (let [resp (http2:send session "GET" "/trailers")]
                   (assert (= resp:status 200) "trailers+body: status 200")
                   (assert (= (string resp:body) "payload")
                           "trailers+body: body")
                   (assert (= resp:trailers:grpc-status "0")
                           "trailers+body: grpc-status")
                   (assert (= resp:trailers:custom-trailer "value")
                           "trailers+body: custom-trailer"))))
  (println "  PASS: trailers with body"))

## ── Test 5: trailers-only (no body) ─────────────────────────────────

(defn test-trailers-only []
  (with-server (fn [req]
                 {:status 200
                  :headers {:content-type "application/grpc"}
                  :trailers [["grpc-status" "0"]]})
               (fn [session]
                 (let [resp (http2:send session "GET" "/trailers-only")]
                   (assert (= resp:status 200) "trailers-only: status 200")
                   (assert (= resp:trailers:grpc-status "0")
                           "trailers-only: grpc-status"))))
  (println "  PASS: trailers-only (no body)"))

## ── Test 6: no trailers (backward compat) ───────────────────────────

(defn test-no-trailers []
  (with-server (fn [req] {:status 200 :body "still works"})
               (fn [session]
                 (let [resp (http2:send session "GET" "/no-trailers")]
                   (assert (= resp:status 200) "no-trailers: status 200")
                   (assert (= (string resp:body) "still works")
                           "no-trailers: body"))))
  (println "  PASS: no trailers (backward compat)"))

## ── Run ────────────────────────────────────────────────────────────────

(println "h2 same-scheduler tests:")
(test-single-request)
(test-sequential-requests)
(test-request-with-body)
(test-trailers-with-body)
(test-trailers-only)
(test-no-trailers)
(println "all h2 same-scheduler tests passed")
