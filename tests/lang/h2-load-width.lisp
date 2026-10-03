(elle/epoch 12)
# One h2 session with many requests in flight at once.
#
# Every request in flight holds a stream id it can never reuse, an entry
# in the stream table, and a slice of the connection's flow-control
# window. Issuing requests one at a time exercises none of that. Issuing
# dozens at once does, and the failure they produce is a stall rather
# than an error — the session stops answering and the caller cannot tell
# a slow server from a wedged one.
#
# So the case below runs wide rounds on one session and then asserts two
# things: every request was answered 200, and the stream table came back
# empty. An entry left behind is a stream the session will never close,
# and it is what turns into a stall a few hundred requests later. Ten
# rounds is what makes the second assertion mean something: a round that
# retires its streams leaves the next round the same window it had.
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

(defn make-handler []
  "A handler with a fixed route and an echoing default."
  (fn [req]
    (let [path req:path]
      (if (= path "/fixed")
        {:status 200 :body "ok"}
        {:status 200 :body (concat "echo:" path)}))))

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

# ── Width: 32 requests in flight at once ─────────────────────────────

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

# ── Run ──────────────────────────────────────────────────────────────

(println "one session with many requests in flight...")

(timed "32 concurrent requests, ten rounds" high-concurrency)

(println "h2 load width: the session answered and left no stream behind")
