(elle/epoch 12)
# One h2 session under a poll loop.
#
# A poll loop reissues the same request until the answer changes, so the
# width of each round depends on what the round before it returned. That
# is a harder shape for a session than a fixed fan-out: the stream ids it
# spends per round vary, and a round only shrinks if the streams of the
# previous round were retired.
#
# So the case below submits 64 jobs, then polls 64 ids concurrently, 32
# at a time, until every one answers "done". It asserts three things:
# every poll was answered 200, no id was still pending when the loop
# ended, and the stream table came back empty. An entry left behind is a
# stream the session will never close, and it is what turns into a stall
# a few hundred requests later.
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
  "A handler with two routes and per-id state.

   `/fixed` returns a short constant and `/status/<id>` answers
   \"pending\" three times per id and \"done\" after that — the shape a
   poll loop needs to make progress."
  (let [@seen @{}]
    (fn [req]
      (let [path req:path]
        (cond
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

(defn timed [label thunk]
  "Run `thunk` under the file's budget and name it."
  (let* [t0 (clock/monotonic)
         r (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? r))
            (string label ": no result in " (string deadline) " s"))
    (println "  " label ": " (string (round elapsed)) " s")
    r))

# ── A poll loop that reissues until the answer changes ───────────────

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

# ── Run ──────────────────────────────────────────────────────────────

(println "one session under a poll loop...")

(timed "64 ids polled concurrently until done" concurrent-polling)

(println "h2 load polling: every id reached done and left no stream behind")
