(elle/epoch 12)
# What `ev/abort` owes the rest of the scheduler.
#
# Aborting a fiber is never a private act. A parked fiber sits in a
# scheduler queue with live fibers behind it, and an in-flight fiber owns
# a submission the backend will complete whether or not anyone still
# wants the result. So every abort has to give two things back without
# taking anything from a fiber that is still running:
#
#   1. **Its place in the park queue.** A wake grants one permit to the
#      fiber at the head. An aborted fiber that stays queued takes that
#      permit and the live waiter behind it never runs.
#   2. **Its submission.** `handle-abort` cancels the operation and drops
#      the pairing, so the id can be reused. Reuse must not deliver the
#      new operation's completion to the old fiber, or the old one's to
#      the new — every other fiber's completion has to arrive intact
#      while cancellations churn.
#
# The four cases below arrange each hazard and assert the live fiber
# still gets what it is owed: a queued producer gets the wake, a
# heartbeat keeps its tick rate through 300 cancellations, and an h2
# session answers a request right after an abort tore an in-flight one
# down — sequentially, then twelve at once.
#
# See docs/scheduler.md § "Park queues" and § "Completion delivery".

(def http2 ((import "std/http2")))
(def sync ((import "std/sync")))

# A budget no unblocked wait here can reach.
(def deadline 8)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of `n` bytes by repeated doubling — O(log n) concats."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

(defn delayed-echo-handler [delay-ms]
  "Echo the request body after `delay-ms`. The delay is what keeps a
   request in flight long enough to abort it mid-response."
  (fn [req]
    (ev/sleep (/ delay-ms 1000.0))
    {:status 200 :body (or req:body (bytes))}))

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
      (test-fn session))))

# ── 1. The wake reaches the waiter behind the aborted one ────────────

(println "a wake passes over an aborted waiter to the live one behind it...")

(let [q (sync:make-queue 1)]
  (q:put "fill")
  # Both producers park on the queue's not-full condition, p1 at the head.
  (def p1
    (ev/spawn (fn []
                (q:put "p1")
                :p1-done)))
  (def p2
    (ev/spawn (fn []
                (q:put "p2")
                :p2-done)))
  (ev/sleep 0.05)
  (ev/abort p1)
  (ev/sleep 0.02)
  (assert (= (q:take) "fill") "the queue held the item that filled it")
  # One take issues one not-full wake, and p2 is the only fiber that can
  # use it.
  (let [r (ev/timeout deadline (fn [] (ev/join p2)))]
    (assert (not (nil? r)) "the live waiter got the wake")
    (assert (= r :p2-done) "the live waiter ran its own body")))

# ── 2. Cancellation churn leaves an unrelated timer alone ────────────

(println "a heartbeat keeps its rate through 300 cancelled timers...")

(def @ticks 0)
(def heartbeat
  (ev/spawn (fn []
              (forever
                (ev/sleep 0.01)
                (assign ticks (+ ticks 1))))))

(defer
  (protect (ev/abort heartbeat))
  (begin
    (ev/sleep 0.2)
    (each _ in (range 0 300)
      (def victim (ev/spawn (fn [] (ev/sleep 100))))
      (ev/sleep 0.003)  # long enough for the timer to reach the backend
      (ev/abort victim))
    (let [before ticks]
      (ev/sleep 0.5)
      # A 10 ms tick over 0.5 s is about 50; the assertion leaves wide
      # room for scheduling noise and still catches a stalled heartbeat.
      (let [delta (- ticks before)]
        (assert (> delta 10)
                (string "the heartbeat ticked " (string delta)
                        " times in 0.5 s after the churn"))))))

# ── 3. A session answers right after an in-flight abort ──────────────

(println "an h2 session stays usable after 30 in-flight aborts...")

(with-server (delayed-echo-handler 150)
             (fn [session]
               (each i in (range 0 30)
                 (def f
                   (ev/spawn (fn []
                               (http2:send session "POST" "/echo"
                               :body (make-body 2000)))))
                 (ev/sleep 0.04)  # the server is still sleeping: the read is in flight
                 (ev/abort f)
                 (let [r (ev/timeout deadline
                                     (fn [] (http2:send session "GET" "/fixed")))]
                   (assert (not (nil? r))
                           (string "the session answered after abort "
                                   (string i)))
                   (assert (= r:status 200)
                           (string "the post-abort request returned 200, abort "
                                   (string i)))))
               true))

# ── 4. Twelve concurrent aborts leave twelve survivors ───────────────

(println "twelve concurrent aborts leave the other twelve requests intact...")

(with-server (delayed-echo-handler 120)
             (fn [session]
               (each round in (range 0 12)
                 (def keep @[])
                 (def kill @[])
                 (each i in (range 0 24)
                   (def f
                     (ev/spawn (fn []
                                 (http2:send session "GET"
                                 (concat "/r?" (string round) "-" (string i))))))
                   (if (= 0 (mod i 2)) (push kill f) (push keep f)))
                 (ev/sleep 0.04)  # every request is in flight
                 (each f in (freeze kill)
                   (ev/abort f))
                 (let [r (ev/timeout deadline
                                     (fn [] (map ev/join (freeze keep))))]
                   (assert (not (nil? r))
                           (string "the survivors finished, round "
                                   (string round)))
                   (assert (= (length r) 12)
                           (string "twelve survivors, round " (string round)
                                   ", got " (string (length r))))
                   (each resp in r
                     (assert (= resp:status 200)
                             (string "a survivor returned 200, round "
                                     (string round))))))
               true))

(println "sched abort conformance: every abort gave back its place and its operation")
