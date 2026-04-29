#!/usr/bin/env elle
(elle/epoch 9)

# Fiber nesting overhead benchmark
#
# Measures per-request latency with different fiber nesting depths
# to quantify the cost of protect/defer signal propagation through
# fiber chains.  Each I/O signal (SIG_IO) must traverse every fiber
# level twice (out to scheduler, back in on completion), so deeper
# nesting multiplies the number of swap protocols per I/O op.
#
# Modes:
#   A  flat loop           — 1 fiber level  (ev/spawn only)
#   B  protect per I/O     — 2 fiber levels (ev/spawn → protect)
#   C  defer wrapping loop — 2 fiber levels (ev/spawn → defer)
#   D  defer + protect     — 3 fiber levels (ev/spawn → defer → protect)
#
# Usage: elle demos/webserver/bench-fiber.lisp [iterations]

(def n (parse-int (or (get (sys/args) 0) "1000")))

# ── Helpers ──────────────────────────────────────────────────────────

(defn get-port [listener]
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn percentile [sorted-arr p]
  (let* [n (length sorted-arr)
         idx (min (- n 1) (floor (* (/ p 100.0) n)))]
    (get sorted-arr idx)))

# ── Server modes ─────────────────────────────────────────────────────
#
# Each function handles n request-response cycles on a connected
# socket.  The protocol is one line per direction: client sends a
# line, server reads it and writes a line back.
#
# The latencies mutable array is filled with per-iteration times.

(defn server-flat [conn n latencies]
  "Mode A: flat loop.  1 fiber level (ev/spawn only)."
  (var i 0)
  (while (< i n)
    (let [t0 (clock/monotonic)]
      (port/read-line conn)
      (port/write conn "ok\n")
      (put latencies i (* (- (clock/monotonic) t0) 1000.0)))
    (assign i (+ i 1))))

(defn server-protect [conn n latencies]
  "Mode B: protect around each I/O op.  2 fiber levels."
  (var i 0)
  (while (< i n)
    (let [t0 (clock/monotonic)]
      (protect (port/read-line conn))
      (protect (port/write conn "ok\n"))
      (put latencies i (* (- (clock/monotonic) t0) 1000.0)))
    (assign i (+ i 1))))

(defn server-defer [conn n latencies]
  "Mode C: defer wrapping the loop body.  2 fiber levels."
  (defer
    nil
    (var i 0)
    (while (< i n)
      (let [t0 (clock/monotonic)]
        (port/read-line conn)
        (port/write conn "ok\n")
        (put latencies i (* (- (clock/monotonic) t0) 1000.0)))
      (assign i (+ i 1)))))

(defn server-defer-protect [conn n latencies]
  "Mode D: defer + protect (connection-loop shape).  3 fiber levels."
  (defer
    (protect nil)
    (var i 0)
    (while (< i n)
      (let [t0 (clock/monotonic)]
        (let [[ok? _] (protect (port/read-line conn))]
          (unless ok? (break)))
        (protect (port/write conn "ok\n"))
        (put latencies i (* (- (clock/monotonic) t0) 1000.0)))
      (assign i (+ i 1)))))

# ── Test runner ──────────────────────────────────────────────────────

(defn run-test [label server-fn iterations]
  "Start an echo server in the given mode, run a client against it,
   print latency distribution."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         port-num (get-port listener)
         latencies @[]]
    (repeat iterations (push latencies 0.0))
    (let* [server (ev/spawn (fn []
                              (let [conn (tcp/accept listener)]
                                (server-fn conn iterations latencies)
                                (port/close conn))))
           client (ev/spawn (fn []
                              (let [conn (tcp/connect "127.0.0.1" port-num)]
                                (var i 0)
                                (while (< i iterations)
                                  (port/write conn "req\n")
                                  (port/read-line conn)
                                  (assign i (+ i 1)))
                                (port/close conn))))]
      (ev/join server)
      (ev/join client))
    (port/close listener)
    (let [sorted (sort (->list latencies))]
      (let [sorted-arr (->array sorted)]
        (println (string/format (string "  {:<35} p50={:.3f}  p95={:.3f}"
                                        "  p99={:.3f}  max={:.3f} ms") label
                                (percentile sorted-arr 50)
                                (percentile sorted-arr 95)
                                (percentile sorted-arr 99)
                                (get sorted-arr (- (length sorted-arr) 1))))))))

# ── Main ─────────────────────────────────────────────────────────────

(println (string/format "fiber nesting benchmark: {} iterations per test\n" n))

# Warmup (JIT, caches, etc.)
(run-test "(warmup)" server-flat 100)
(println "")

(run-test "A: flat (1 fiber level)" server-flat n)
(run-test "B: protect (2 fiber levels)" server-protect n)
(run-test "C: defer (2 fiber levels)" server-defer n)
(run-test "D: defer+protect (3 levels)" server-defer-protect n)

(println "\ndone")
