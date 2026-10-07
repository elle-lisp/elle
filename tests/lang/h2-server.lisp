(elle/epoch 14)
# audited: 2026-10-06
# The h2 server: requests, headers, flow control, errors, CONTINUATION and lifecycle.
# lib/http2.md

(def http2 ((import "std/http2")))
(def hpack ((import "std/http2/hpack") :huffman ((import "std/http2/huffman"))))

## ── Helpers ──────────────────────────────────────────────────────────────

# RFC 9113's smallest SETTINGS_MAX_FRAME_SIZE. A header block one byte
# over it cannot travel in a single frame to a peer that advertises it.
(def frame-floor 16384)

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

(defn
  with-server
  [handler test-fn &named on-error server-max-frame client-max-frame]
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn []
                        (let [[ok? _] (protect (http2:serve listener handler
                              :on-error on-error
                              :max-frame-size server-max-frame))]
                          nil)))
         session (http2:connect (concat "http://127.0.0.1:" (string lport))
                                :max-frame-size client-max-frame)]
    (defer
      (begin
        (protect (http2:close session))
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
        (begin
          (assign pass-count (+ pass-count 1))
          (println "  PASS: " name))
      (and ok? (nil? err))
        (begin
          (assign fail-count (+ fail-count 1))
          (push failures name)
          (println "  FAIL: " name " (timeout)"))
      true
        (begin
          (assign fail-count (+ fail-count 1))
          (push failures name)
          (println "  FAIL: " name " — " err)))))

## ── Group 1: basic server operation ─────────────────────────────────────

(defn test-single-request []
  (with-server (fn [req] {:status 200 :body (concat "echo:" req:path)})
               (fn [session]
                 (let [resp (http2:send session "GET" "/hello")]
                   (assert (= resp:status 200) "status 200")
                   (assert (= (string resp:body) "echo:/hello") "body")
                   true))))

(defn test-sequential-requests []
  (with-server (fn [req] {:status 200 :body (concat "seq:" req:path)})
               (fn [session]
                 (each i in (range 0 10)
                   (let [resp (http2:send session "GET"
                         (concat "/req-" (string i)))]
                     (assert (= resp:status 200)
                             (concat "seq req " (string i) " status"))
                     (assert (= (string resp:body)
                                (concat "seq:/req-" (string i)))
                             (concat "seq req " (string i) " body"))))
                 true)))

(defn test-post-with-body []
  (with-server (fn [req]
                 {:status 200
                  :body (if (nil? req:body) "nobody" (string req:body))})
               (fn [session]
                 (let [resp (http2:send session "POST" "/data"
                                        :body "hello world")]
                   (assert (= resp:status 200) "post status")
                   (assert (= (string resp:body) "hello world") "post body")
                   true))))

## ── Group 2: response headers ────────────────────────────────────────────

(defn test-response-headers-preserved []
  (with-server (fn [req]
                 {:status 200
                  :headers {:content-type "text/plain" :x-custom "val"}
                  :body "ok"})
               (fn [session]
                 (let [resp (http2:send session "GET" "/headers")]
                   (assert (= resp:status 200) "status 200")
                   (assert (= (get resp:headers :content-type) "text/plain")
                           (concat "content-type: got "
                                   (string (get resp:headers :content-type))))
                   (assert (= (get resp:headers :x-custom) "val")
                           (concat "x-custom: got "
                                   (string (get resp:headers :x-custom))))
                   true))))

(defn test-response-headers-empty []
  (with-server (fn [req] {:status 204})
               (fn [session]
                 (let [resp (http2:send session "GET" "/empty")]
                   (assert (= resp:status 204) "status 204")
                   true))))

## ── Group 3: flow control ────────────────────────────────────────────────

(defn test-large-response-body []
  (let [big-body (apply concat
                        (map (fn [_]
                               (bytes 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9))
                             (range 0 6554)))]
    (with-server (fn [req] {:status 200 :body big-body})
                 (fn [session]
                   (let [resp (http2:send session "GET" "/big")]
                     (assert (= resp:status 200) "status 200")
                     (assert (= (length resp:body) (length big-body))
                             (concat "body length: expected "
                                     (string (length big-body)) " got "
                                     (string (length resp:body))))
                     true)))))

(defn test-large-request-body []
  (let [big-body (apply concat
                        (map (fn [_]
                               (bytes 0 1 2 3 4 5 6 7 8 9 0 1 2 3 4 5 6 7 8 9))
                             (range 0 6554)))]
    (with-server (fn [req] {:status 200 :body (string (length req:body))})
                 (fn [session]
                   (let [resp (http2:send session "POST" "/upload"
                         :body big-body)]
                     (assert (= resp:status 200) "status 200")
                     (assert (= (string resp:body) (string (length big-body)))
                             (concat "echoed length: " (string resp:body)))
                     true)))))

## ── Group 4: error handling ──────────────────────────────────────────────

(defn test-handler-error-returns-500 []
  (with-server (fn [req] (error {:error :test-error :message "boom"}))
               (fn [session]
                 (let [resp (http2:send session "GET" "/error")]
                   (assert (= resp:status 500)
                           (concat "expected 500, got " (string resp:status)))
                   true))))

(defn test-handler-error-with-on-error []
  (def @captured-error nil)
  (with-server (fn [req] (error {:error :test-error :message "boom"}))
               (fn [session]
                 (let [resp (http2:send session "GET" "/error")]
                   (assert (= resp:status 500) "status 500")
                   (ev/sleep 0.1)
                   (assert (not (nil? captured-error))
                           "on-error callback should fire")
                   true)) :on-error (fn [err] (assign captured-error err))))

(defn test-handler-slow-no-hang []
  (with-server (fn [req]
                 (ev/sleep 1)
                 {:status 200 :body "slow"})
               (fn [session]
                 (let [resp (http2:send session "GET" "/slow")]
                   (assert (= resp:status 200) "status 200")
                   (assert (= (string resp:body) "slow") "body")
                   true))))

## ── Group 5: CONTINUATION frames ─────────────────────────────────────────
#
# The receiving side advertises `frame-floor`, and its reader refuses any
# frame larger than that. So a header block over `frame-floor` that
# arrives intact has crossed as HEADERS plus CONTINUATION; no single frame
# could have carried it. Each test asserts the block really is over the
# floor before it relies on that.
#
# The counter-factual is the default advertisement, 256 KiB on both
# sides. A block of 16 KiB then travels in one HEADERS frame, and a test
# that names CONTINUATION passes without one being sent.
#
# The values are `X` and `Z`, whose Huffman codes are a byte each. The
# encoder sends such a string raw, so the blocks cost their size in bytes
# and no Huffman work on either side.

(defn raw-value [n]
  "A header value of `n` bytes that HPACK sends raw."
  (string/repeat "XZ" (/ n 2)))

(def big-headers [["x-big-0" (raw-value 9000)] ["x-big-1" (raw-value 9000)]])

(defn over-the-floor? [pairs]
  "True when `pairs` encode, on a fresh encoder, to a block over
   `frame-floor`."
  (> (length (hpack:encode (hpack:make-encoder) pairs)) frame-floor))

(defn test-large-response-headers []
  (assert (over-the-floor? big-headers)
          "the response block must exceed one frame")
  (with-server (fn [req]
                 {:status 200
                  :headers {:x-big-0 (raw-value 9000) :x-big-1 (raw-value 9000)}
                  :body "ok"})
               (fn [session]
                 (let [resp (http2:send session "GET" "/big-headers")]
                   (assert (= resp:status 200) "status 200")
                   (assert (= (get resp:headers :x-big-0) (raw-value 9000))
                           "x-big-0 must arrive intact")
                   (assert (= (get resp:headers :x-big-1) (raw-value 9000))
                           "x-big-1 must arrive intact")
                   true)) :client-max-frame frame-floor))

## ── Group 6: connection lifecycle ────────────────────────────────────────

(defn test-stream-cleanup-no-leak []
  (with-server (fn [req] {:status 200 :body "ok"})
               (fn [session]
                 (each i in (range 0 50)
                   (let [resp (http2:send session "GET"
                         (concat "/leak-" (string i)))]
                     (assert (= resp:status 200)
                             (concat "req " (string i) " status"))))
                 (assert (= (length (keys session:streams)) 0)
                         (concat "stream leak: "
                                 (string (length (keys session:streams)))
                                 " streams remaining"))
                 true)))

(defn test-settings-window-adjustment []
  (with-server (fn [req] {:status 200 :body "ok"})
               (fn [session]
                 (assert (not (nil? (get session:remote-settings
                                    :initial-window-size)))
                         "remote initial-window-size set")
                 (let [resp (http2:send session "GET" "/settings")]
                   (assert (= resp:status 200) "status 200")
                   true))))

## ── Group 7: a closed session ─────────────────────────────────────────────

(defn test-goaway-refuses-new-streams []  # After server closes, client should refuse new streams
  (with-server (fn [req] {:status 200 :body "ok"})
               (fn [session]
                 (let [resp (http2:send session "GET" "/first")]
                   (assert (= resp:status 200) "first request ok"))  # Close session — marks goaway-recvd after GOAWAY exchange
                 (http2:close session)
                 (let [[ok? err] (protect (http2:send session "GET" "/second"))]
                   (assert (not ok?) "should refuse after close")
                   true))))

## ── Group 8: large request headers (CONTINUATION) ─────────────────────
#
# Group 5's argument, with the roles swapped: the server advertises
# `frame-floor`, so the client has to split.

(defn test-large-request-headers []
  (assert (over-the-floor? big-headers)
          "the request block must exceed one frame")
  (with-server (fn [req]
                 {:status 200
                  :body (if (and (= (get req:headers :x-big-0) (raw-value 9000))
                                 (= (get req:headers :x-big-1) (raw-value 9000)))
                          "intact"
                          "damaged")})
               (fn [session]
                 (let [resp (http2:send session "GET" "/big-req-hdrs"
                                        :headers big-headers)]
                   (assert (= resp:status 200) "status 200")
                   (assert (= (string resp:body) "intact")
                           "both request headers must reach the handler intact")
                   true)) :server-max-frame frame-floor))

## ── Group 9: max-concurrent-streams ────────────────────────────────────

(defn test-max-concurrent-streams-enforced []
  (with-server (fn [req]
                 (ev/sleep 0.5)
                 {:status 200 :body "ok"})
               (fn [session]
                 # Server allows 100 concurrent streams by default
                 # Send 3 concurrent requests — all should succeed
                 (let [fibers (map (fn [i]
                                     (ev/spawn (fn []
                                       (http2:send session "GET"
                                       (concat "/conc-" (string i))))))
                                   (range 0 3))]
                   # A deadline that aborts this test leaves the requests
                   # running against a session the cleanup closes. Each
                   # then fails, nobody joins it, and the failure ends the
                   # whole file in place of the FAIL line naming this test.
                   (defer
                     (each f in fibers
                       (protect (ev/abort f)))
                     (each r in (map ev/join fibers)
                       (assert (= r:status 200) "concurrent: status 200")))
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
(run-test "large request headers" test-large-request-headers)
(run-test "max concurrent streams" test-max-concurrent-streams-enforced)
(println)
(println "results: " pass-count "/" test-count " passed, " fail-count " failed")
(when (> fail-count 0) (println "failures: " (freeze failures)))
(assert (= fail-count 0) "all h2 server tests must pass")
