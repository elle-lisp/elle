(elle/epoch 12)
# One h2 session under sustained request load.
#
# A session accumulates state per request: a stream id it can never
# reuse, an entry in the stream table, a slice of the connection's
# flow-control window, and whatever the peer's SETTINGS left outstanding.
# A single request exercises none of that. Hundreds on one connection do,
# and the failure they produce is a stall rather than an error — the
# session stops answering and the caller cannot tell a slow server from a
# wedged one.
#
# So each case below drives one session hard along a different axis and
# then asserts two things: every request was answered 200, and the stream
# table came back empty. An entry left behind is a stream the session
# will never close, and it is what turns into a stall a few hundred
# requests later.
#
# The axes: body size, concurrency width, connection churn, poll loops
# that reissue the same request until it changes answer, the two mixed,
# and plain volume on one connection.
#
# See lib/http2/session.lisp and docs/scheduler.md.

(def http2 ((import "std/http2")))

# A budget no unblocked case here can reach.
(def deadline 60)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of `n` bytes by repeated doubling — O(log n) concats. Building
   it one chunk at a time is quadratic and outgrows the budget above long
   before the h2 work does."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

(defn make-handler []
  "A handler with four routes and per-id state.

   `/echo` returns the request body, `/fixed` a short constant, and
   `/status/<id>` answers \"pending\" three times per id and \"done\"
   after that — the shape a poll loop needs to make progress."
  (let [@seen @{}]
    (fn [req]
      (let [path req:path]
        (cond
          (= path "/echo")
            {:status 200 :body (or req:body (bytes ""))}
          (= path "/fixed") {:status 200 :body "ok"}
          (string/starts-with? path "/status/")
            (let* [id (slice path 8)
                   n (or (get seen id) 0)]
              (put seen id (+ n 1))
              (if (< n 3)
                {:status 200 :body "pending"}
                {:status 200 :body "done"}))
          true {:status 200 :body (concat "echo:" path)})))))

(defn with-server [handler test-fn]
  "Serve `handler` on a fresh listener and call `test-fn` with a session."
  (let* [[listener lport] (listen-ephemeral)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
         session (http2:connect (concat "http://127.0.0.1:" (string lport)))]
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (test-fn session lport))))

(defn no-stream-leak [session label]
  "The stream table is empty once every request has been answered."
  (assert (= (length (keys session:streams)) 0)
          (string label ": the stream table came back empty")))

(defn parallel-get [session paths]
  "Issue every path at once and join the results in order."
  (map ev/join
       (map (fn [p] (ev/spawn (fn [] (http2:send session "GET" p)))) paths)))

(defn all-200 [results label]
  "Every response in `results` is a 200."
  (each r in results
    (assert (= r:status 200) (string label ": every response was 200"))))

(defn timed [label thunk]
  "Run `thunk` under the file's budget and name it."
  (let* [t0 (clock/monotonic)
         r (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? r))
            (string label ": no result in " (string deadline) " s"))
    (println "  " label ": " (string (round elapsed)) " s")
    r))

# ── 1. Volume with a large body on every request ─────────────────────

(defn sequential-large-bodies []
  "200 requests of 50 KB, one after another on one session."
  (let [body (make-body 50000)]
    (with-server (make-handler)
                 (fn [session _]
                   (each i in (range 0 200)
                     (let [resp (http2:send session "POST" "/echo" :body body)]
                       (assert (= resp:status 200)
                               (string "large bodies: request " (string i)))
                       (assert (= (length resp:body) (length body))
                               (string "large bodies: request " (string i)
                                       " echoed the whole body"))))
                   (no-stream-leak session "large bodies")
                   true))))

# ── 2. Width: 32 requests in flight at once ──────────────────────────

(defn high-concurrency []
  "32 requests at once, ten rounds, on one session."
  (with-server (make-handler)
               (fn [session _]
                 (each round in (range 0 10)
                   (let [results (parallel-get session
                         (map (fn [i]
                                (concat "/fixed?r=" (string round) "&i="
                                        (string i))) (range 0 32)))]
                     (all-200 results "concurrency")
                     (assert (= (length results) 32)
                             (string "concurrency: 32 results in round "
                                     (string round)))))
                 (no-stream-leak session "concurrency")
                 true)))

# ── 3. Connection churn under load ───────────────────────────────────

(defn reconnect-cycles []
  "Twenty times: connect, send 50 requests, close."
  (let* [[listener lport] (listen-ephemeral)
         handler (make-handler)
         sf (ev/spawn (fn [] (protect (http2:serve listener handler))))
         url (concat "http://127.0.0.1:" (string lport))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 20)
        (let [session (http2:connect url)]
          (each i in (range 0 50)
            (let [resp (http2:send session "GET"
                                   (concat "/fixed?c=" (string cycle) "&i="
                                   (string i)))]
              (assert (= resp:status 200)
                      (string "reconnect: cycle " (string cycle) " request "
                              (string i)))))
          (http2:close session)))
      true)))

# ── 4. A poll loop that reissues until the answer changes ────────────

(defn concurrent-polling []
  "Submit 64 requests, then poll 64 ids concurrently, 32 at a time,
   until every one answers \"done\"."
  (with-server (make-handler)
               (fn [session _]
                 (each i in (range 0 64)
                   (let [resp (http2:send session "POST" "/fixed" :body "submit")]
                     (assert (= resp:status 200)
                             (string "polling: submit " (string i)))))
                 (let [@pending (->list (map (fn [i] (string i)) (range 0 64)))
                       @rounds 0]
                   # The handler turns each id "done" on its fourth poll, so this
                   # converges well inside the bound; the bound is what keeps a
                   # handler that never converges from running to the budget.
                   (while (and (not (empty? pending)) (< rounds 20))
                     (let [@next @[]
                           ids (->array pending)
                           n (length ids)
                           batches (if (<= n 32)
                                     [(->list ids)]
                                     [(->list (slice ids 0 32))
                                      (->list (slice ids 32))])]
                       (each batch in batches
                         (let [results (map ev/join
                               (map (fn [id]
                                      (ev/spawn (fn []
                                        [id
                                        (http2:send session "GET"
                                        (concat "/status/" id))]))) batch))]
                           (each result in results
                             (let [id (get result 0)
                                   resp (get result 1)]
                               (assert (= resp:status 200)
                                       (string "polling: id " id))
                               (when (= (string resp:body) "pending")
                                 (push next id))))))
                       (assign pending next))
                     (assign rounds (+ rounds 1)))
                   (assert (empty? pending)
                           (string "polling: " (string (length pending))
                                   " ids still pending after " (string rounds)
                                   " rounds")))
                 (no-stream-leak session "polling")
                 true)))

# ── 5. Large sequential sends interleaved with concurrent ones ───────

(defn large-then-concurrent []
  "Five cycles of: 20 sequential 50 KB requests, then 20 at once."
  (let [body (make-body 50000)]
    (with-server (make-handler)
                 (fn [session _]
                   (each cycle in (range 0 5)
                     (each i in (range 0 20)
                       (let [resp (http2:send session "POST" "/echo" :body body)]
                         (assert (= resp:status 200)
                                 (string "interleave: cycle " (string cycle)
                                 " large request " (string i)))))
                     (let [results (parallel-get session
                           (map (fn [i]
                                  (concat "/fixed?c=" (string cycle) "&i="
                                  (string i))) (range 0 20)))]
                       (all-200 results "interleave")
                       (assert (= (length results) 20)
                               (string "interleave: 20 results in cycle "
                                       (string cycle)))))
                   (no-stream-leak session "interleave")
                   true))))

# ── 6. Plain volume on one connection ────────────────────────────────

(defn session-durability []
  "500 requests on one session, 10 KB in and 10 KB out."
  (let [request-body (make-body 10000)
        response-body (make-body 10000)]
    (with-server (fn [req] {:status 200 :body response-body})
                 (fn [session _]
                   (each i in (range 0 500)
                     (let [resp (http2:send session "POST" "/echo"
                           :body request-body)]
                       (assert (= resp:status 200)
                               (string "durability: request " (string i)))))
                   (no-stream-leak session "durability")
                   true))))

# ── Run ──────────────────────────────────────────────────────────────

(println "one session under sustained load...")

(timed "200 sequential requests of 50 KB" sequential-large-bodies)
(timed "32 concurrent requests, ten rounds" high-concurrency)
(timed "20 connect/close cycles of 50 requests" reconnect-cycles)
(timed "64 ids polled concurrently until done" concurrent-polling)
(timed "20 large then 20 concurrent, five cycles" large-then-concurrent)
(timed "500 requests on one session" session-durability)

(println "h2 stress load: every session answered and left no stream behind")
