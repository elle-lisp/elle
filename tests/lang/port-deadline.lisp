(elle/epoch 14)
# audited: 2026-09-30
## A :deadline bounds a whole port call, and a port's own :timeout applies to each call that names none.
## docs/io/timeout.md
##
## `:timeout` bounds each kernel operation, so a peer that is slow but keeps
## sending carries a looping read past it (port-read-timeout.lisp case 5).
## `:deadline` bounds the whole call, so the same peer runs the same read into
## it. That pair is the counter-factual: a deadline that were only a longer
## timeout would let the transfer finish.
##
## Each peer that could end a call on its own does so seconds after the bound,
## so `elapsed` separates "ended at its bound" from "ended because the peer
## did". Both backends run this file.

(def late 2.0)  # s, above this a call did not end at its own bound

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn timed [thunk]
  "Run `thunk` under protect. Returns [ok? result elapsed-seconds]."
  (let* [started (clock/monotonic)
         [ok? result] (protect (thunk))
         elapsed (- (clock/monotonic) started)]
    [ok? result elapsed]))

(defn assert-timed-out [label outcome]
  "Assert the call ended with a :timeout error, well before `late`."
  (let [[ok? result elapsed] outcome]
    (assert (< elapsed late)
            (concat label ": ran " (string elapsed)
                    "s — the call waited for its peer instead of its bound"))
    (assert (not ok?)
            (concat label ": expected a :timeout, got " (string result)))
    (assert (= (get result :error) :timeout)
            (concat label ": expected a :timeout error, got " (string result)))))

(defn with-peer [serve body]
  "Run `serve` on the accepted end of a fresh TCP connection and `body` on the
   connecting end. Returns what `body` returns."
  (ev/run (fn []
            (let [listener (tcp/listen "127.0.0.1" 0)]
              (ev/spawn (fn []
                          (let [conn (tcp/accept listener)]
                            (serve conn)
                            (port/close conn))))
              (let [conn (tcp/connect "127.0.0.1" (listen-port listener)
                                      :sndbuf 4096 :timeout 5)]
                (defer
                  (begin
                    (port/close conn)
                    (port/close listener))
                  (body conn)))))))

(defn slow-sender [conn]
  "Send ten 4 KiB chunks 0.15 s apart, then linger."
  (repeat 10 (port/write conn (bytes (string/repeat "z" 4096))) (ev/sleep 0.15))
  (ev/sleep 3))

(defn quiet [conn]
  "Hold the connection open and send nothing."
  (ev/sleep 3))

(defn late-line [conn]
  "Send one line 0.6 s in, then linger."
  (ev/sleep 0.6)
  (port/write conn "hello\n")
  (ev/sleep 3))

## ── 1. :deadline ends the transfer :timeout lets finish ─────────────

(with-peer slow-sender
           (fn [conn]
             (let [got (port/read-exact conn (* 4096 10) :timeout 1)]
               (assert (= (length got) (* 4096 10))
                       "a :timeout longer than every gap lets the transfer finish"))))

(with-peer slow-sender
           (fn [conn]
             (let* [until (+ (clock/monotonic) 0.4)
                    outcome (timed (fn []
                                     (port/read-exact conn (* 4096 10)
                                     :deadline until)))]
               (assert-timed-out "read-exact with :deadline" outcome)
               (assert (>= (clock/monotonic) until)
                       "the read ended before its :deadline"))))

(println "  1. :deadline ends a looping read that :timeout lets finish")

## ── 2. :deadline ends a write to a slow reader ──────────────────────

(defn slow-reader [conn]
  "Read 256 KiB every 0.1 s until the other end closes."
  (forever
    (let [chunk (port/read-exact conn 262144)]
      (when (nil? chunk) (break))
      (ev/sleep 0.1))))

(with-peer slow-reader
           (fn [conn]
             (let* [until (+ (clock/monotonic) 0.4)
                    outcome (timed (fn []
                                     (port/write conn
                                     (bytes (string/repeat "x" (* 12 1024 1024)))
                                     :timeout 1 :deadline until)))]
               (assert-timed-out "write with :timeout and :deadline" outcome)
               (assert (>= (clock/monotonic) until)
                       "the write ended before its :deadline"))))

(println "  2. :deadline ends a write whose every operation beats :timeout")

## ── 3. With both bounds, the earlier ends the call ──────────────────

(with-peer quiet
           (fn [conn]
             (assert-timed-out "read-line, :timeout before :deadline"
                               (timed (fn []
                                        (port/read-line conn :timeout 0.2
                                        :deadline (+ (clock/monotonic) 10)))))))

(println "  3. a :timeout before the :deadline ends the call")

## ── 4. A deadline already past answers what is ready ────────────────

(with-peer (fn [conn]
             (port/write conn "ready\n")
             (ev/sleep 3))
           (fn [conn]
             (ev/sleep 0.2)
             (let [got (port/read-line conn :deadline (- (clock/monotonic) 1))]
               (assert (= got "ready")
                       (concat "a past deadline still answers the line that is ready, got "
                               (string got))))
             (assert-timed-out "read with a past deadline and nothing ready"
                               (timed (fn []
                                        (port/read conn 64
                                        :deadline (- (clock/monotonic) 1)))))))

(println "  4. a past deadline answers what is ready, else times out")

## ── 5. A port's own :timeout ────────────────────────────────────────

(with-peer late-line
           (fn [conn]
             (port/set-options conn :timeout 0.2)
             (assert-timed-out "read-line under the port's :timeout"
                               (timed (fn [] (port/read-line conn))))
             (port/set-options conn :timeout nil)
             (assert (= (port/read-line conn) "hello")
                     ":timeout nil removes the port's bound, so the read waits")))

(with-peer late-line
           (fn [conn]
             (port/set-options conn :timeout 0.2)
             (assert (= (port/read-line conn :timeout 2) "hello")
                     "a call's own :timeout replaces the port's")))

(with-peer late-line
           (fn [conn]
             (port/set-options conn :timeout 2)
             (let* [until (+ (clock/monotonic) 0.2)
                    outcome (timed (fn [] (port/read-line conn :deadline until)))]
               (assert-timed-out "read-line with a :deadline under the port's :timeout"
                                 outcome)
               (assert (>= (clock/monotonic) until)
                       "the read ended before its :deadline"))))

(println "  5. a port's own :timeout applies to each call that names none")

## ── 6. Pipe peers ───────────────────────────────────────────────────
##
## A pipe takes no socket options, so its bounds belong to the operation.
## Each child exits after 3 s, which closes its end: an unbounded call returns
## then with EOF or EPIPE, so `elapsed` is again the discriminator.

(defn with-child [body]
  "Run `body` on a child that neither reads nor writes for 3 s."
  (ev/run (fn []
            (let [child (subprocess/exec "sleep" ["3"])]
              (defer
                (begin
                  (protect (subprocess/kill child :sigterm))
                  (protect (subprocess/wait child)))
                (body child))))))

(with-child (fn [child]
              (assert-timed-out "pipe read with :deadline"
                                (timed (fn []
                                         (port/read (get child :stdout) 64
                                         :deadline (+ (clock/monotonic) 0.2)))))))

(with-child (fn [child]
              (assert-timed-out "pipe write with :deadline"
                                (timed (fn []
                                         (port/write (get child :stdin)
                                         (bytes (string/repeat "x" 1000000))
                                         :deadline (+ (clock/monotonic) 0.2)))))))

(with-child (fn [child]
              (let [out (get child :stdout)]
                (port/set-options out :timeout 0.2)
                (assert-timed-out "pipe read under the port's :timeout"
                                  (timed (fn [] (port/read out 64)))))))

(println "  6. pipe peers end at their :deadline and their port's :timeout")

## ── 7. The calls that wait for a peer take :deadline ────────────────

(ev/run (fn []
          (let [listener (tcp/listen "127.0.0.1" 0)]
            (defer
              (port/close listener)
              (assert-timed-out "tcp/accept with :deadline"
                                (timed (fn []
                                         (tcp/accept listener
                                         :deadline (+ (clock/monotonic) 0.2)))))))))

(ev/run (fn []
          (let [sock (udp/bind "127.0.0.1" 0)]
            (defer
              (port/close sock)
              (assert-timed-out "udp/recv-from with :deadline"
                                (timed (fn []
                                         (udp/recv-from sock 64
                                         :deadline (+ (clock/monotonic) 0.2)))))))))

(println "  7. accept and receive end at their :deadline")

## ── 8. Bad bounds are refused, not waited out ───────────────────────

(defn refused? [thunk]
  "True when `thunk` signals at once with an error other than :timeout."
  (let [[ok? err elapsed] (timed thunk)]
    (and (not ok?) (not (= (get err :error) :timeout)) (< elapsed late))))

(with-child (fn [child]
              (let [out (get child :stdout)]
                (assert (refused? (fn [] (port/read out 64 :timeout -1)))
                        "a negative :timeout is refused")
                (assert (refused? (fn [] (port/read out 64 :timeout "soon")))
                        "a :timeout that is not a number is refused")
                (assert (refused? (fn [] (port/read out 64 :deadline "soon")))
                        "a :deadline that is not a number is refused")
                (assert (refused? (fn [] (port/set-options out :timeout -1)))
                        "port/set-options refuses a negative :timeout")
                (assert (refused? (fn [] (port/set-options out :timeout :soon)))
                        "port/set-options refuses a :timeout that is not a number"))))

(println "  8. bad bounds are refused")

## ── 9. A bound further off than the clock can count bounds nothing ──
##
## 1e300 seconds is past what any clock counts. The call waits as long as it
## takes, exactly as one that names no bound does: the late line arrives and
## the read answers it. The trap: that duration does not fit a Rust Duration,
## and the conversion that assumed it would took the whole process down.

(with-peer late-line
           (fn [conn]
             (assert (= (port/read-line conn :timeout 1e300) "hello")
                     "a :timeout beyond the clock waits for the line")))

(with-peer late-line
           (fn [conn]
             (port/set-options conn :timeout 1e300)
             (assert (= (port/read-line conn) "hello")
                     "a port :timeout beyond the clock waits for the line")))

(ev/run (fn []
          (let [[tx rx] (chan)]
            (ev/spawn (fn []
                        (ev/sleep 0.2)
                        (chan/send tx :late)))
            (assert (= (chan/select @[rx] :timeout 1e300) [0 :late])
                    "a select with a :timeout beyond the clock waits for the message"))))

(println "  9. a bound beyond the clock waits as long as it takes")

(println "port-deadline: all tests passed")
