(elle/epoch 13)
# audited: 2026-09-28
# Timed bidi streams over an h2 session that keeps getting replaced, varying the recycle.
# tests/AGENTS.md
# docs/concurrency.md
# docs/scheduler.md
#
# `ev/timeout` races the work against a timer and aborts whichever loses,
# so every call tears a fiber down — twice per call, counting the timer.
# A bidi stream leaves a reader parked on the stream's data queue, and
# recycling the session closes the connection that reader sits on. Run
# the two together and the teardown of one stream overlaps the setup of
# the next.
#
# What must hold across all of it is that the session still answers. Each
# case runs legs of bidi streams under a budget, with a recycle between
# legs, and finishes with a unary request under its own budget. A session
# whose reader, stream table or flow-control window did not survive the
# teardown answers that request with a hang rather than a 200.
#
# The unary request runs on the session that carried the last leg, where
# the torn-down readers sat. A case that sets `:fresh-session` recycles
# once more first, so its unary request meets a session no stream touched.
# The counter-factual: a driver that always recycles before the unary
# request never asks a session that carried streams for anything more.
#
# The cases here vary the RECYCLE: how many streams sit either side of
# one, how large their messages are, and how tightly the recycles follow
# each other. h2-timeout-serving.lisp varies what the SERVER does. The
# two are separate files because each is a whole program under a
# wall-clock budget, and the thread-pool I/O backend that every non-Linux
# build uses runs them slowest.
#
# The last section puts the same `ev/timeout` churn on a bare queue with
# no h2 under it, so a failure there separates the scheduler from the
# protocol.

(def http2 ((import "std/http2")))
(def sync ((import "std/sync")))

# A budget no unblocked stream here can reach.
(def deadline 30)

# A budget no unblocked unary request here can reach.
(def unary-budget 10)

(defn listen-ephemeral []
  "A listening socket on a kernel-chosen port, with that port."
  (let* [l (tcp/listen "127.0.0.1" 0)
         p (port/path l)
         port (parse-int (slice p (+ 1 (string/find p ":"))))]
    [l port]))

# ── gRPC framing ─────────────────────────────────────────────────────

(defn grpc-frame [payload]
  "Prefix `payload` with the gRPC message header: a compression byte and
   a 4-byte big-endian length."
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

(def body-chunk (bytes 0 1 2 3 4 5 6 7 8 9 10 11 12 13 14 15 16 17 18 19))

(defn make-body [n]
  "A body of `n` bytes by repeated doubling — O(log n) concats. Building
   it one chunk at a time is quadratic and outgrows the budget above long
   before the h2 work does."
  (let [@b body-chunk]
    (while (< (length b) n) (assign b (concat b b)))
    (slice b 0 n)))

# ── Handler ──────────────────────────────────────────────────────────

(defn echo-handler [req]
  "Echo the request body with gRPC trailers."
  {:status 200
   :headers {:content-type "application/grpc"}
   :body (or req:body (bytes))
   :trailers [["grpc-status" "0"]]})

# ── One bidi exchange ────────────────────────────────────────────────

(defn do-bidi [session n-msgs msg-size]
  "Send `n-msgs` framed messages of `msg-size` bytes on one stream and
   require the echo to return every one of them."
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
    (assert (= received n-msgs)
            (string "the echo returned " (string received) " of "
                    (string n-msgs) " messages"))
    received))

# ── The driver every case shares ─────────────────────────────────────

(defn run-recycles [opts]
  "Serve the echo handler, then run `opts:cycles` legs of `opts:streams`
   bidi streams under `opts:budget`, closing the session and reconnecting
   between legs. Finish with a unary request under `unary-budget` on the
   session that carried the last leg, or on a fresh one when
   `opts:fresh-session` is set."
  (let* [label opts:label
         cycles (or (get opts :cycles) 1)
         streams (or (get opts :streams) 5)
         msgs (or (get opts :msgs) 5)
         size (or (get opts :size) 500)
         budget (or (get opts :budget) deadline)
         [listener lport] (listen-ephemeral)
         url (concat "http://127.0.0.1:" (string lport))
         sf (ev/spawn (fn [] (protect (http2:serve listener echo-handler))))
         @session (http2:connect url)]
    (defn recycle []
      "Close the session the streams just torn down were reading, and
       reconnect."
      (http2:close session)
      (assign session (http2:connect url)))
    (defer
      (begin
        (protect (http2:close session))
        (protect (port/close listener))
        (protect (ev/abort sf)))
      (each cycle in (range 0 cycles)
        (when (> cycle 0) (recycle))
        (each i in (range 0 streams)
          (let [r (ev/timeout budget (fn [] (do-bidi session msgs size)))]
            (assert (not (nil? r))
                    (string label ": stream " (string i) " of cycle "
                            (string cycle) " reached its budget")))))
      (when (get opts :fresh-session) (recycle))
      (let [resp (ev/timeout unary-budget
                             (fn [] (http2:send session "GET" "/health")))]
        (assert (not (nil? resp))
                (string label ": the unary request reached its budget"))
        (assert (= resp:status 200)
                (string label ": the unary request after the last leg")))
      true)))

(defn run-case [label opts]
  "Run one case and name it on the way past."
  (println "  " label)
  (run-recycles (merge opts {:label label})))

# ── The cases ────────────────────────────────────────────────────────

(println "timed bidi streams across session recycles...")

(run-case "5 streams either side of one recycle"
          {:cycles 2 :streams 5 :msgs 5 :size 500})

(run-case "20 messages of 2 KB, ten streams per leg"
          {:cycles 2 :streams 10 :msgs 20 :size 2000})

(run-case "recycle after every two streams, ten times"
          {:cycles 10 :streams 2 :msgs 10 :size 1000 :fresh-session true})

(run-case "twenty recycles, three streams each"
          {:cycles 20 :streams 3 :msgs 5 :size 500 :fresh-session true})

# ── The same churn with no h2 under it ───────────────────────────────

(println "the same timeout churn on a bare queue...")

(let [q (sync:make-queue 8)]
  (each i in (range 0 100)
    (ev/timeout 5
                (fn []
                  (q:put (string i))
                  (assert (= (q:take) (string i))
                          (string "the queue returned its own item, round "
                                  (string i))))))
  (q:put "final")
  (assert (= (q:take) "final")
          "the queue still works after 100 timed put/take rounds"))

(println "  a queue survives 100 timed put/take rounds")

(let [@q (sync:make-queue 16)]
  (defn produce [start]
    "A producer that fills `q` until it is aborted."
    (ev/spawn (fn []
                (def @n start)
                (forever
                  (q:put (string n))
                  (assign n (+ n 1))
                  (ev/sleep 0.001)))))
  (def @producer (produce 0))
  (each _ in (range 0 50)
    (assert (not (nil? (ev/timeout 5 (fn [] (q:take)))))
            "a take from the first producer finished in time"))
  (ev/abort producer)
  # Replace the queue and its producer wholesale, the way a session
  # recycle replaces a connection and its reader.
  (assign q (sync:make-queue 16))
  (assign producer (produce 1000))
  (each _ in (range 0 50)
    (assert (not (nil? (ev/timeout 5 (fn [] (q:take)))))
            "a take from the replacement producer finished in time"))
  (ev/abort producer))

(println "  a queue survives having its producer replaced")

(println "h2 timeout recycle: every session answered after its teardown")
