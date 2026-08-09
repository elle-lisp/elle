(elle/epoch 12)
# A timed-out bidi stream, caught, over a recycled session.
#
# Three mechanisms meet in a gRPC client and each one tears fibers down:
# `ev/timeout` aborts the loser of its race on every call, a bidi stream
# leaves a reader parked on the session's data queue, and recycling the
# session closes the connection those readers are parked on. `protect`
# around the timeout adds the fourth: the caller catches the timeout's
# error instead of unwinding, so the fibers the timeout aborted are torn
# down while their caller keeps running.
#
# The invariant is that the session stays usable through all of it. Each
# phase below runs bidi streams under `ev/timeout`, recycles the session,
# runs more, and closes with a unary request — the request is the check,
# because a session whose reader or flow-control state did not survive
# the teardown answers it with a hang rather than a 200.
#
# The second phase is the one that adds `protect`. It also doubles the
# message count and size, so the timed-out streams are still moving bytes
# when the recycle closes the connection under them.
#
# See docs/concurrency.md § ev/timeout and lib/http2/session.lisp.

(def http2 ((import "std/http2")))

# A budget no unblocked phase here can reach. Reported on expiry so a
# slow run reads differently from a wedged one.
(def deadline 30)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

(defn within [name thunk]
  "Run `thunk` under the file's budget. Names the phase and its elapsed
   time, so a phase that merely got slow is distinguishable from one that
   stopped making progress."
  (let* [t0 (clock/monotonic)
         result (ev/timeout deadline thunk)
         elapsed (- (clock/monotonic) t0)]
    (assert (not (nil? result))
            (string name ": no result in " (string deadline) " s"))
    (println "  " name ": " (string (round elapsed)) " s")
    result))

# ── gRPC framing ─────────────────────────────────────────────────────
#
# Length-prefixed messages: one compression byte, then a 4-byte
# big-endian length. The reader below needs the framing because a bidi
# response arrives as DATA of arbitrary size, not one frame per read.

(defn grpc-frame [payload]
  "Prefix `payload` with the gRPC message header."
  (let [len (length payload)]
    (concat (bytes 0 (bit/shr len 24) (bit/and (bit/shr len 16) 0xff)
                   (bit/and (bit/shr len 8) 0xff) (bit/and len 0xff)) payload)))

(defn grpc-read-frame [buf]
  "Split one framed message off `buf`, or nil if it holds less than one."
  (when (>= (length buf) 5)
    (let [len (bit/or (bit/shl (get buf 1) 24) (bit/shl (get buf 2) 16)
                      (bit/shl (get buf 3) 8) (get buf 4))
          end (+ 5 len)]
      (when (>= (length buf) end) [(slice buf 5 end) (slice buf end)]))))

# The 20-byte unit every payload repeats.
(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of `n` bytes by repeated doubling — O(log n) concats. Building
   it one chunk at a time is quadratic and dominates the budget above
   long before the h2 work does."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

# ── One bidi exchange ────────────────────────────────────────────────

(defn do-bidi [session n-msgs msg-size]
  "Send `n-msgs` framed messages of `msg-size` bytes on one stream and
   count the framed messages the echo returns."
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
  "Echo the request body with gRPC trailers."
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (or req:body (bytes))
   :trailers [["grpc-status" "0"]]})

(defn with-session [body-fn]
  "Serve `echo-handler` on a fresh listener and call `body-fn` with a
   connect function. Closes the server and its socket on the way out."
  (let* [[listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))]
    (defer
      (begin
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (body-fn (fn [] (http2:connect url))))))

(defn unary-check [session label]
  "A unary request answered 200 is the proof the session still works."
  (let [resp (http2:send session "GET" "/health")]
    (assert (= resp:status 200) (string label ": unary request after recycle"))))

# ── Phase 1: timed-out bidi, uncaught, across a recycle ──────────────

(println "timed-out bidi streams across a session recycle...")

(defn cycle-uncaught [n]
  "Run `n` bidi streams under `ev/timeout`, recycle, run `n` more."
  (with-session (fn [connect]
                  (let [@session (connect)]
                    (defer
                      (protect (http2:close session))
                      (each _ in (range 0 n)
                        (ev/timeout deadline (fn [] (do-bidi session 5 500))))
                      (http2:close session)
                      (assign session (connect))
                      (each _ in (range 0 n)
                        (ev/timeout deadline (fn [] (do-bidi session 5 500))))
                      (unary-check session "uncaught")
                      true)))))

(each round in (range 0 3)
  (within (string "uncaught round " (string round)) (fn [] (cycle-uncaught 5))))

# ── Phase 2: the same, with the timeout's error caught ───────────────

(println "the same with protect around every timeout...")

(defn cycle-caught [n msgs size]
  "Run `n` bidi streams under `protect`-wrapped `ev/timeout`, recycle,
   run `n` more. `protect` keeps the caller running past a teardown
   instead of unwinding with it."
  (with-session (fn [connect]
                  (let [@session (connect)]
                    (defer
                      (protect (http2:close session))
                      (each i in (range 0 n)
                        (let [[ok? _] (protect (ev/timeout deadline
                              (fn [] (do-bidi session msgs size))))]
                          (assert ok? (string "caught: stream " (string i)))))
                      (http2:close session)
                      (assign session (connect))
                      (each i in (range n (* 2 n))
                        (let [[ok? _] (protect (ev/timeout deadline
                              (fn [] (do-bidi session msgs size))))]
                          (assert ok? (string "caught: stream " (string i)))))
                      (unary-check session "caught")
                      true)))))

(within "caught, 10 streams of 10 × 1000 B either side of the recycle"
        (fn [] (cycle-caught 10 10 1000)))

(println "h2 timeout protect: the session outlived every teardown")
