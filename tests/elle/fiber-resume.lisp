#!/usr/bin/env elle
(elle/epoch 9)

# Tests for fiber resume through nested FiberResume chains.
#
# Exercises the direct fiber resumption optimization: when the scheduler
# resumes a fiber chain (ev/spawn → defer → protect), values must flow
# correctly through all levels even when intermediate swap protocols
# are skipped.

# ── Setup: TCP echo server ─────────────────────────────────────────

(def listener (tcp/listen "127.0.0.1" 0))
(def port-num (parse-int (get (string/split (port/path listener) ":") 1)))

(def server-fiber
  (ev/spawn (fn []
              (forever
                (let [[ok? conn] (protect (tcp/accept listener))]
                  (unless ok? (break nil))
                  (ev/spawn (fn []
                              (defer (port/close conn)
                                     (forever
                                       (let [line (port/read-line conn)]
                                         (when (nil? line) (break nil))
                                         (port/write conn
                                         (string "echo:" (string line) "\n"))))))))))))

# ── Test 1: protect around I/O inside defer ────────────────────────
#
# 3 fiber levels: ev/spawn → defer → protect
# Each port/read-line inside protect suspends through the full chain.

(println "  1. defer+protect around TCP I/O...")
(let [result @[nil]]
  (let [f (ev/spawn (fn []
                      (let [conn (tcp/connect "127.0.0.1" port-num)]
                        (defer (port/close conn)
                               (let [[ok? _] (protect (port/write conn "hello\n"))]
                                 (assert ok? "write succeeded"))
                               (let [[ok? line] (protect (port/read-line conn))]
                                 (assert ok? "read succeeded")
                                 (put result 0 (string line)))))))]
    (ev/join f))
  (assert (= (get result 0) "echo:hello")
          "value flows correctly through defer+protect fiber chain"))
(println "  1. ok")

# ── Test 2: multiple sequential I/O ops inside protect ─────────────
#
# Each protect wraps multiple reads/writes. Each I/O op suspends and
# resumes through the full chain.

(println "  2. multiple I/O inside protect...")
(let [result @[nil nil nil]]
  (let [f (ev/spawn (fn []
                      (let [conn (tcp/connect "127.0.0.1" port-num)]
                        (defer (port/close conn)
                               (let [[ok? lines] (protect
                                       (port/write conn "a\n")
                                       (port/write conn "b\n")
                                       (port/write conn "c\n")
                                       (let [r1 (string (port/read-line conn))
                                         r2 (string (port/read-line conn))
                                         r3 (string (port/read-line conn))]
                                         [r1 r2 r3]))]
                                 (assert ok? "multi-I/O protect succeeded")
                                 (put result 0 (get lines 0))
                                 (put result 1 (get lines 1))
                                 (put result 2 (get lines 2)))))))]
    (ev/join f))
  (assert (= (get result 0) "echo:a") "first read correct")
  (assert (= (get result 1) "echo:b") "second read correct")
  (assert (= (get result 2) "echo:c") "third read correct"))
(println "  2. ok")

# ── Test 3: keepalive loop (connection-loop shape) ─────────────────
#
# Exactly the defer+protect pattern from connection-loop:
# (defer cleanup (forever (protect read) (protect write)))

(println "  3. connection-loop shape...")
(let [results @[]]
  (let [f (ev/spawn (fn []
                      (let [conn (tcp/connect "127.0.0.1" port-num)]
                        (defer (protect (port/close conn))
                               (var i 0)
                               (while (< i 5)
                                 (let [[ok? _] (protect (port/write conn
                                       (string i "\n")))]
                                   (unless ok? (break)))
                                 (let [[ok? line] (protect (port/read-line conn))]
                                   (unless ok? (break))
                                   (push results (string line)))
                                 (assign i (+ i 1)))))))]
    (ev/join f))
  (assert (= (length results) 5) "5 round-trips completed")
  (assert (= (get results 0) "echo:0") "round-trip 0")
  (assert (= (get results 4) "echo:4") "round-trip 4"))
(println "  3. ok")

# ── Test 4: error inside protect propagates correctly ──────────────
#
# An error inside protect (3 levels deep) must not be lost or
# corrupted by the optimization.

(println "  4. error through nested fibers...")
(let [result @[nil nil]]
  (let [f (ev/spawn (fn []
                      (defer nil
                             (let [[ok? val] (protect (error {:reason :test-error}))]
                               (put result 0 ok?)
                               (put result 1 val:reason)))))]
    (ev/join f))
  (assert (= (get result 0) false) "error captured by protect")
  (assert (= (get result 1) :test-error) "error value preserved"))
(println "  4. ok")

# ── Test 5: nested defer+protect (4 fiber levels) ──────────────────
#
# ev/spawn → outer defer → inner defer → protect
# Tests that the optimization chains correctly through multiple levels.

(println "  5. nested defer (4 fiber levels)...")
(let [result @[nil]]
  (let [f (ev/spawn (fn []
                      (let [conn (tcp/connect "127.0.0.1" port-num)]
                        (defer (port/close conn)
                               (defer nil
                                      (let [[ok? _] (protect (port/write conn
                                        "deep\n"))]
                                        (assert ok? "deep write"))
                                      (let [[ok? line] (protect (port/read-line conn))]
                                        (assert ok? "deep read")
                                        (put result 0 (string line))))))))]
    (ev/join f))
  (assert (= (get result 0) "echo:deep")
          "value flows through 4-level fiber chain"))
(println "  5. ok")

# ── Cleanup ────────────────────────────────────────────────────────

(port/close listener)

(println "  all fiber-resume tests passed")
