(elle/epoch 12)
# One h2 session under plain request volume.
#
# A session accumulates state per request: a stream id it can never
# reuse, an entry in the stream table, a slice of the connection's
# flow-control window, and whatever the peer's SETTINGS left outstanding.
# A single request exercises none of that. Hundreds on one connection do,
# and the failure they produce is a stall rather than an error — the
# session stops answering and the caller cannot tell a slow server from a
# wedged one.
#
# The case below is the plainest shape that reaches those numbers: 500
# requests on one connection, one after another, with a body in each
# direction. It asserts two things: every request was answered 200, and
# the stream table came back empty. An entry left behind is a stream the
# session will never close, and it is what turns into a stall a few
# hundred requests later.
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

(defn timed [label thunk]
  "Run `thunk` under the file's budget and name it."
  (let* [t0 (clock/monotonic)
         r (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? r))
            (string label ": no result in " (string deadline) " s"))
    (println "  " label ": " (string (round elapsed)) " s")
    r))

# ── Plain volume on one connection ───────────────────────────────────

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

(println "one session under plain request volume...")

(timed "500 requests on one session" session-durability)

(println "h2 load volume: the session answered and left no stream behind")
