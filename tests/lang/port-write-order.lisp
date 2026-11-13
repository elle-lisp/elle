(elle/epoch 15)
# audited: 2026-11-12
## Writes to one port go out one at a time, each whole, and a write waits in line.
## docs/io/write.md
## docs/io/timeout.md
##
## Two fibers that write to one socket each hand port/write a whole payload.
## One write(2) moves only what fits in the send buffer, so each write takes
## many syscalls, and nothing below the backend keeps the two apart. A 4 KiB
## send buffer makes every payload here take hundreds of them.
##
## The writes reach the backend in the order the scheduler runs the fibers,
## which this file does not fix. So case 1 asks that each payload arrive whole,
## not which comes first.
##
## Case 2 is the counter-factual for the line itself: a write sent at once,
## beside the one ahead of it, parks on a full send buffer and trips its own
## :timeout. A write that waits in line does not start its :timeout until it
## starts. Cases 3 and 4 pin what a write that leaves the line sends: nothing.
## Case 5 puts a port/write-many in the same line as a port/write. Case 6
## changes a mutable part while its write waits in line: the peer must read
## the part as the call was given it, which a copy taken when the write starts
## would miss.
## The thread-pool build runs this file too, so both backends answer it.

## ── Helpers ──────────────────────────────────────────────────────────

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn connected-pair []
  "A listener, a connection to it with a 4 KiB send buffer, and the peer the
   listener accepted. The peer reads only when a case asks it to."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         conn (tcp/connect "127.0.0.1" (listen-port listener) :sndbuf 4096
                           :timeout 5)
         peer (tcp/accept listener :timeout 5)]
    [listener conn peer]))

(defn run-of [letter n]
  "`n` bytes, each the one byte of `letter`."
  (bytes (string/repeat letter n)))

## A payload past what the send buffer and the peer's receive buffer hold
## together, so the write ahead is still running while a case's later write
## waits behind it.
(def big (* 4 1024 1024))

(defn write-ahead [conn]
  "Spawn a fiber that writes `big` bytes of a to `conn`, and let it reach the
   backend before the caller's next write does."
  (let [f (ev/spawn (fn [] (port/write conn (run-of "a" big))))]
    (ev/sleep 0.05)
    f))

(defn assert-only-the-write-ahead [conn peer ahead what]
  "Read the write ahead's bytes, close `conn`, and assert the peer then reads
   end of stream: nothing of the write that left the line arrived."
  (let [whole (= (port/read-exact peer big :timeout 5) (run-of "a" big))]
    (assert whole (string what ": the write ahead must arrive whole")))
  (assert (= (ev/join ahead) big)
          (string what ": the write ahead reports its payload"))
  (port/close conn)
  (assert (nil? (port/read peer 16))
          (string what ": the write that left the line sent bytes")))

## ── 1. Four writers to one socket, each payload whole ────────────────

(ev/run (fn []
          (let* [[listener conn peer] (connected-pair)
                 size 200000
                 payloads (map (fn [l] (run-of l size)) ["a" "b" "c" "d"])
                 writers (map (fn [p] (ev/spawn (fn [] (port/write conn p))))
                              payloads)
                 got (port/read-exact peer (* 4 size) :timeout 5)
                 seen @||]
            (each n in (ev/join writers)
              (assert (= n size) "each writer reports its whole payload"))
            (each i in (range 4)
              (let* [block (slice got (* i size) (* (+ i 1) size))
                     which (find-index (fn [p] (= p block)) payloads)]
                (assert (not (nil? which))
                        (string "block " i
                                " of what the peer received mixes payloads"))
                (add seen which)))
            (assert (= (length seen) 4) "each payload arrives exactly once")
            (port/close peer)
            (port/close conn)
            (port/close listener))))

(println "  1. four writers to one socket: each payload arrives whole")

## ── 2. A waiting write's :timeout starts when it starts ──────────────

(ev/run (fn []
          (let* [[listener conn peer] (connected-pair)
                 ahead (write-ahead conn)
                 behind (ev/spawn (fn []
                                    (port/write conn (run-of "b" 1000)
                                      :timeout 0.2)))]
            ## The peer reads nothing for longer than the waiting write's
            ## timeout, while that write waits in line.
            (ev/sleep 0.5)
            (let* [[read? got] (protect (port/read-exact peer (+ big 1000)
                                          :timeout 5))
                   [ok? answer] (ev/join-protected behind)]
              (assert ok? (string "the write waiting in line failed: " answer))
              (assert read? (string "the peer never received both writes: " got))
              (assert (= answer 1000) "and reports its whole payload")
              (assert (= (ev/join ahead) big)
                      "the write ahead reports its payload")
              (let [in-order (= got (concat (run-of "a" big) (run-of "b" 1000)))]
                (assert in-order
                        "the peer receives the write ahead, then the one behind")))
            (port/close peer)
            (port/close conn)
            (port/close listener))))

(println "  2. a waiting write's :timeout starts when the write starts")

## ── 3. A write cancelled in line sends nothing ───────────────────────

(ev/run (fn []
          (let* [[listener conn peer] (connected-pair)
                 ahead (write-ahead conn)]
            (assert (nil? (ev/timeout 0.2
                                      (fn [] (port/write conn (run-of "b" 1000)))))
                    "ev/timeout ends the write waiting in line")
            (assert-only-the-write-ahead conn peer ahead
              "a cancelled waiting write")
            (port/close peer)
            (port/close listener))))

(println "  3. a write cancelled in line sends nothing")

## ── 4. A deadline that passes in line ends the write unsent ──────────

(ev/run (fn []
          (let* [[listener conn peer] (connected-pair)
                 ahead (write-ahead conn)
                 [ok? err] (protect (port/write conn (run-of "b" 1000)
                                      :deadline (+ (clock/monotonic) 0.2)))]
            (assert (not ok?) "a write whose deadline passes in line fails")
            (assert (= (get err :error) :timeout)
                    "and fails as a timed-out call")
            (assert-only-the-write-ahead conn peer ahead
              "a waiting write past its deadline")
            (port/close peer)
            (port/close listener))))

(println "  4. a deadline that passes in line ends the write unsent")

## ── 5. A gathered write waits in the same line ───────────────────────

(ev/run (fn []
          (let* [[listener conn peer] (connected-pair)
                 ahead (write-ahead conn)
                 behind (ev/spawn (fn []
                                    (port/write-many conn
                                      [(run-of "b" 1000) (run-of "c" 1000)])))
                 got (port/read-exact peer (+ big 2000) :timeout 5)
                 in-order (= got
                             (concat (run-of "a" big) (run-of "b" 1000)
                                     (run-of "c" 1000)))]
            (assert (= (ev/join behind) 2000)
                    "the gathered write reports every byte of its parts")
            (assert (= (ev/join ahead) big)
                    "the write ahead reports its payload")
            (assert in-order
                    "the gathered write goes after the write ahead, its parts in order")
            (port/close peer)
            (port/close conn)
            (port/close listener))))

(println "  5. a gathered write waits in the same line")

## ── 6. A mutable part changed in line leaves as the call gave it ──────

(ev/run (fn []
          (let* [[listener conn peer] (connected-pair)
                 ahead (write-ahead conn)
                 store (thaw (run-of "b" 1000))
                 parts @[(run-of "c" 1000)]
                 one (ev/spawn (fn [] (port/write conn store)))
                 many (ev/spawn (fn [] (port/write-many conn parts)))]
            ## Both writes now wait behind the write ahead.
            (ev/sleep 0.05)
            (each i in (range 1000)
              (put store i 120))
            (append store (bytes "more"))
            (put parts 0 (run-of "d" 1000))
            (let* [got (port/read-exact peer (+ big 2000) :timeout 5)
                   a (run-of "a" big)
                   b (run-of "b" 1000)
                   c (run-of "c" 1000)
                   as-given (or (= got (concat a b c)) (= got (concat a c b)))]
              (assert (= (ev/join one) 1000)
                      "the @bytes write reports its payload")
              (assert (= (ev/join many) 1000)
                      "the @array write reports its part")
              (assert (= (ev/join ahead) big)
                      "the write ahead reports its payload")
              (assert as-given
                      "the writes waiting in line sent parts changed after the call"))
            (port/close peer)
            (port/close conn)
            (port/close listener))))

(println "  6. a mutable part changed in line leaves as the call gave it")

(println "port-write-order: all tests passed")
