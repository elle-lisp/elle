(elle/epoch 12)
# One h2 session under a load of large request bodies.
#
# A body larger than a DATA frame is split across frames and spends the
# connection's flow-control window until a WINDOW_UPDATE returns the
# credit. Send hundreds of them on one session and the window, not the
# request, becomes the thing that can stall: the session stops answering
# and the caller cannot tell a slow server from a wedged one.
#
# Both cases here drive 50 KB bodies through one session and then assert
# two things: every request was answered 200, and the stream table came
# back empty. An entry left behind is a stream the session will never
# close, and it is what turns into a stall a few hundred requests later.
#
# The second case puts concurrent requests between the large ones, so the
# window has to serve a wide round and a deep one in turn.
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
  "A handler with two routes: `/echo` returns the request body and
   `/fixed` a short constant."
  (fn [req]
    (let [path req:path]
      (cond
        (= path "/echo")
          {:status 200 :body (or req:body (bytes ""))}
        (= path "/fixed") {:status 200 :body "ok"}
        true {:status 200 :body (concat "echo:" path)}))))

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

# ── 2. Large sequential sends interleaved with concurrent ones ───────

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

# ── Run ──────────────────────────────────────────────────────────────

(println "one session under a load of large bodies...")

(timed "200 sequential requests of 50 KB" sequential-large-bodies)
(timed "20 large then 20 concurrent, five cycles" large-then-concurrent)

(println "h2 load bodies: every session answered and left no stream behind")
