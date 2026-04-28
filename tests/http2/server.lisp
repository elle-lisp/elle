(elle/epoch 9)
## tests/http2/server.lisp — comprehensive HTTP/2 server behavior tests

(def http2 ((import "std/http2")))

## ── Helpers ──────────────────────────────────────────────────────────────

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn with-server [handler test-fn &named on-error]
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn
           (fn []
             (let [[ok? _] (protect
               (http2:serve listener handler :on-error on-error))]
               nil)))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer (begin (protect (http2:close session))
                  (protect (port/close listener))
                  (protect (ev/abort sf)))
      (test-fn session))))

(def @test-count 0)
(def @pass-count 0)
(def @fail-count 0)
(def @failures @[])

(defn run-test [name thunk]
  (assign test-count (+ test-count 1))
  (let [[ok? err] (protect (ev/timeout 10 thunk))]
    (cond
      (and ok? (not (nil? err)))
       (begin (assign pass-count (+ pass-count 1))
              (println "  PASS: " name))
      (and ok? (nil? err))
       (begin (assign fail-count (+ fail-count 1))
              (push failures name)
              (println "  FAIL: " name " (timeout)"))
      true
       (begin (assign fail-count (+ fail-count 1))
              (push failures name)
              (println "  FAIL: " name " — " err)))))

## ── Group 1: basic server operation ─────────────────────────────────────

(defn test-single-request []
  (with-server
    (fn [req] {:status 200 :body (concat "echo:" req:path)})
    (fn [session]
      (let [resp (http2:send session "GET" "/hello")]
        (assert (= resp:status 200) "status 200")
        (assert (= (string resp:body) "echo:/hello") "body")
        true))))

(defn test-sequential-requests []
  (with-server
    (fn [req] {:status 200 :body (concat "seq:" req:path)})
    (fn [session]
      (each i in (range 0 10)
        (let [resp (http2:send session "GET" (concat "/req-" (string i)))]
          (assert (= resp:status 200)
                  (concat "seq req " (string i) " status"))
          (assert (= (string resp:body) (concat "seq:/req-" (string i)))
                  (concat "seq req " (string i) " body"))))
      true)))

(defn test-post-with-body []
  (with-server
    (fn [req]
      {:status 200
       :body (if (nil? req:body) "nobody" (string req:body))})
    (fn [session]
      (let [resp (http2:send session "POST" "/data" :body "hello world")]
        (assert (= resp:status 200) "post status")
        (assert (= (string resp:body) "hello world") "post body")
        true))))

## ── Group 2: response headers ────────────────────────────────────────────

(defn test-response-headers-preserved []
  (with-server
    (fn [req]
      {:status 200
       :headers {:content-type "text/plain" :x-custom "val"}
       :body "ok"})
    (fn [session]
      (let [resp (http2:send session "GET" "/headers")]
        (assert (= resp:status 200) "status 200")
        (assert (= (get resp:headers :content-type) "text/plain")
                (concat "content-type: got " (string (get resp:headers :content-type))))
        (assert (= (get resp:headers :x-custom) "val")
                (concat "x-custom: got " (string (get resp:headers :x-custom))))
        true))))

(defn test-response-headers-empty []
  (with-server
    (fn [req] {:status 204})
    (fn [session]
      (let [resp (http2:send session "GET" "/empty")]
        (assert (= resp:status 204) "status 204")
        true))))

## ── Group 3: flow control ────────────────────────────────────────────────

(defn test-large-response-body []
  (let [big-body (apply concat (map (fn [_] (bytes 0 1 2 3 4 5 6 7 8 9
                                                   0 1 2 3 4 5 6 7 8 9))
                                    (range 0 6554)))]
    (with-server
      (fn [req] {:status 200 :body big-body})
      (fn [session]
        (let [resp (http2:send session "GET" "/big")]
          (assert (= resp:status 200) "status 200")
          (assert (= (length resp:body) (length big-body))
                  (concat "body length: expected " (string (length big-body))
                          " got " (string (length resp:body))))
          true)))))

(defn test-large-request-body []
  (let [big-body (apply concat (map (fn [_] (bytes 0 1 2 3 4 5 6 7 8 9
                                                   0 1 2 3 4 5 6 7 8 9))
                                    (range 0 6554)))]
    (with-server
      (fn [req]
        {:status 200
         :body (string (length req:body))})
      (fn [session]
        (let [resp (http2:send session "POST" "/upload" :body big-body)]
          (assert (= resp:status 200) "status 200")
          (assert (= (string resp:body) (string (length big-body)))
                  (concat "echoed length: " (string resp:body)))
          true)))))

## ── Group 4: error handling ──────────────────────────────────────────────

(defn test-handler-error-returns-500 []
  (with-server
    (fn [req] (error {:error :test-error :message "boom"}))
    (fn [session]
      (let [resp (http2:send session "GET" "/error")]
        (assert (= resp:status 500)
                (concat "expected 500, got " (string resp:status)))
        true))))

(defn test-handler-error-with-on-error []
  (def @captured-error nil)
  (with-server
    (fn [req] (error {:error :test-error :message "boom"}))
    (fn [session]
      (let [resp (http2:send session "GET" "/error")]
        (assert (= resp:status 500) "status 500")
        (ev/sleep 0.1)
        (assert (not (nil? captured-error))
                "on-error callback should fire")
        true))
    :on-error (fn [err] (assign captured-error err))))

(defn test-handler-slow-no-hang []
  (with-server
    (fn [req]
      (ev/sleep 1)
      {:status 200 :body "slow"})
    (fn [session]
      (let [resp (http2:send session "GET" "/slow")]
        (assert (= resp:status 200) "status 200")
        (assert (= (string resp:body) "slow") "body")
        true))))

## ── Group 5: CONTINUATION frames ─────────────────────────────────────────

(defn test-large-response-headers []
  (let [hdrs @{}]
    (each i in (range 0 200)
      (put hdrs (keyword (concat "x-hdr-" (string i)))
           (apply concat (map (fn [_] "abcdefghij") (range 0 10)))))
    (with-server
      (fn [req] {:status 200 :headers (freeze hdrs) :body "ok"})
      (fn [session]
        (let [resp (http2:send session "GET" "/big-headers")]
          (assert (= resp:status 200) "status 200")
          (assert (= (get resp:headers :x-hdr-0)
                     (apply concat (map (fn [_] "abcdefghij") (range 0 10))))
                  "x-hdr-0 value")
          true)))))

## ── Group 6: connection lifecycle ────────────────────────────────────────

(defn test-stream-cleanup-no-leak []
  (with-server
    (fn [req] {:status 200 :body "ok"})
    (fn [session]
      (each i in (range 0 50)
        (let [resp (http2:send session "GET" (concat "/leak-" (string i)))]
          (assert (= resp:status 200)
                  (concat "req " (string i) " status"))))
      (assert (= (length (keys session:streams)) 0)
              (concat "stream leak: " (string (length (keys session:streams)))
                      " streams remaining"))
      true)))

(defn test-settings-window-adjustment []
  (with-server
    (fn [req] {:status 200 :body "ok"})
    (fn [session]
      (assert (not (nil? (get session:remote-settings :initial-window-size)))
              "remote initial-window-size set")
      (let [resp (http2:send session "GET" "/settings")]
        (assert (= resp:status 200) "status 200")
        true))))

## ── Group 7: LOW defects ──────────────────────────────────────────────────

(defn test-goaway-refuses-new-streams []
  # After server closes, client should refuse new streams
  (with-server
    (fn [req] {:status 200 :body "ok"})
    (fn [session]
      (let [resp (http2:send session "GET" "/first")]
        (assert (= resp:status 200) "first request ok"))
      # Close session — marks goaway-recvd after GOAWAY exchange
      (http2:close session)
      (let [[ok? err] (protect (http2:send session "GET" "/second"))]
        (assert (not ok?) "should refuse after close")
        true))))

## ── Run ──────────────────────────────────────────────────────────────────

(println "h2 server tests:")
(run-test "single request" test-single-request)
(run-test "sequential requests" test-sequential-requests)
(run-test "POST with body" test-post-with-body)
(run-test "response headers preserved" test-response-headers-preserved)
(run-test "response headers empty (204)" test-response-headers-empty)
(run-test "large response body (128KB)" test-large-response-body)
(run-test "large request body (128KB)" test-large-request-body)
(run-test "handler error returns 500" test-handler-error-returns-500)
(run-test "handler error with on-error" test-handler-error-with-on-error)
(run-test "handler slow no hang" test-handler-slow-no-hang)
(run-test "large response headers" test-large-response-headers)
(run-test "stream cleanup no leak" test-stream-cleanup-no-leak)
(run-test "SETTINGS window adjustment" test-settings-window-adjustment)
(run-test "GOAWAY refuses new streams" test-goaway-refuses-new-streams)
(println)
(println "results: " pass-count "/" test-count " passed, " fail-count " failed")
(when (> fail-count 0)
  (println "failures: " (freeze failures)))
(assert (= fail-count 0) "all h2 server tests must pass")
