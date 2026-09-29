(elle/epoch 12)
# HTTP/2 session recycle under a deadline, with deferred cleanup.
#
# One cycle couples four mechanisms that each own a piece of the
# scheduler's state:
#
#   - an outer `ev/timeout`, which arms a timer for the whole cycle;
#   - bidi streams driven from `each`, which desugars to a fiber, so
#     every stream runs one level deeper than the cycle body;
#   - a session closed and reopened inside the scope, so the handle the
#     `defer` cleans up is not the handle the scope opened;
#   - a `defer` that closes the recycled session, closes the listener,
#     and aborts the fiber running `http2:serve`.
#
# Three properties hold across that shape:
#
#   1. The cycle body returns its value. The outer `ev/timeout` yields
#      that value, not the nil it yields when its deadline wins.
#   2. A unary request still reaches the server after the recycle.
#   3. Timers keep running once the deferred cleanup has finished. Each
#      cleanup call parks its fiber rather than holding the OS thread,
#      so a later `ev/timeout` still fires on a body that outlives its
#      deadline.
#
# The cycle runs in two arms. The second puts a `protect` between each
# stream's `ev/timeout` and the stream, so a catch frame stands between
# the timer that would abort the stream and the stream itself.
#
# The arms are written out rather than shared through a helper. What
# this file covers is a shape, and the shape includes how deep each
# `each` and each `ev/timeout` sits — routing the streams through a
# common function moves them, and then the cycle under test is a
# different one.

(def http2 ((import "std/http2")))

# ── Helpers ──────────────────────────────────────────────────────────

(defn listen-ephemeral []
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))]
    [listener lport]))

# The 20-byte unit every payload repeats.
(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "Body of `n` bytes by repeated doubling — O(log n) concats."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

(defn grpc-frame [p]
  "Wrap `p` in a gRPC length-prefixed message: one flag byte, four
   big-endian length bytes, then the payload."
  (let [len (length p)]
    (concat (bytes 0 (bit/shr len 24) (bit/and (bit/shr len 16) 0xff)
                   (bit/and (bit/shr len 8) 0xff) (bit/and len 0xff)) p)))

(defn grpc-read-frame [buf]
  "Split one gRPC message off the front of `buf`, or nil when `buf`
   holds less than a whole message. Returns [payload rest]."
  (when (>= (length buf) 5)
    (let [len (bit/or (bit/shl (get buf 1) 24) (bit/shl (get buf 2) 16)
                      (bit/shl (get buf 3) 8) (get buf 4))
          end (+ 5 len)]
      (when (>= (length buf) end) [(slice buf 5 end) (slice buf end)]))))

(defn do-bidi [session n-msgs msg-size]
  "Send `n-msgs` gRPC messages of `msg-size` bytes on one stream, then
   read the echoed messages back. Returns how many came back."
  (let [framed (grpc-frame (make-body msg-size))]
    (def [sid s]
      (http2:open-stream session "POST" "/test.Svc/Method"
                         :headers [["content-type" "application/grpc"]
                                   ["te" "trailers"]]))
    (each _ in (range 0 n-msgs)
      (http2:stream-send session sid framed))
    (http2:stream-end session sid)
    (def @buf (bytes))
    (def @received 0)
    (def @done false)
    (while (not done)
      (let [msg (s:data-queue:take)]
        (match msg:type
          :headers (when msg:end-stream (assign done true))
          :data
            (begin
              (assign buf (concat buf msg:data))
              (when msg:end-stream (assign done true)))
          _ (assign done true)))
      (while true
        (let [r (grpc-read-frame buf)]
          (if (nil? r)
            (break nil)
            (begin
              (assign received (+ received 1))
              (assign buf (get r 1)))))))
    received))

(defn echo-handler [req]
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (or req:body (bytes))
   :trailers [["grpc-status" "0"]]})

# ── Arm one: the plain cycle ─────────────────────────────────────────

(defn run-one-cycle [n]
  "Run `n` bidi streams, recycle the session, run `n` more, then one
   unary request. Returns true when the cycle finished inside its
   deadline. The deadline is two orders of magnitude above what the
   cycle costs, so only a wedged cycle can reach it."
  (let [result (ev/timeout 30
                           (fn []
                             (let* [[listener lport] (listen-ephemeral)
                                    url (concat "http://127.0.0.1:"
                                    (string lport))
                                    sf (ev/spawn (fn []
                                      (protect (http2:serve listener
                                      echo-handler))))
                                    @session (http2:connect url)]
                               (defer
                                 (begin
                                   (protect (http2:close session))
                                   (protect (port/close listener))
                                   (protect (ev/abort sf)))
                                 (each i in (range 0 n)
                                   (ev/timeout 30
                                   (fn [] (do-bidi session 5 500))))
                                 (http2:close session)
                                 (assign session (http2:connect url))
                                 (each i in (range 0 n)
                                   (ev/timeout 30
                                   (fn [] (do-bidi session 5 500))))
                                 (let [resp (http2:send session "GET" "/health")]
                                   (assert (= resp:status 200)
                                   "plain: unary failed"))
                                 true))))]
    (not (nil? result))))

(println "recycle cycles, plain...")
(each i in (range 0 3)
  (assert (run-one-cycle 5) (string "plain cycle " i " did not return")))

# ── Arm two: a protect between each timer and its stream ─────────────

(println "recycle cycles, protect around each stream...")

(let [result (ev/timeout 30
                         (fn []
                           (let* [[listener lport] (listen-ephemeral)
                                  url (concat "http://127.0.0.1:" (string lport))
                                  sf (ev/spawn (fn []
                                    (protect (http2:serve listener echo-handler))))
                                  @session (http2:connect url)]
                             (defer
                               (begin
                                 (protect (http2:close session))
                                 (protect (port/close listener))
                                 (protect (ev/abort sf)))
                               (each i in (range 0 10)
                                 (let [[ok? r] (protect (ev/timeout 30
                                       (fn [] (do-bidi session 10 1000))))]
                                   (assert ok?
                                   (string "guarded: stream " i " raised"))))
                               (http2:close session)
                               (assign session (http2:connect url))
                               (each i in (range 10 20)
                                 (let [[ok? r] (protect (ev/timeout 30
                                       (fn [] (do-bidi session 10 1000))))]
                                   (assert ok?
                                   (string "guarded: stream " i " raised"))))
                               (let [resp (http2:send session "GET" "/health")]
                                 (assert (= resp:status 200)
                                 "guarded: unary failed"))
                               true))))]
  (assert (not (nil? result)) "guarded cycle did not return"))

# Property 3: the cleanup left the scheduler able to run a timer. A
# body that sleeps far past its deadline must still lose to the timer.
(println "timer after cleanup...")
(assert (nil? (ev/timeout 0.01 (fn [] (ev/sleep 100))))
        "timeout does not fire after a recycle cycle's cleanup")

(println "h2 recycle cleanup: all cycles returned")
