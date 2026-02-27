# Time and Clock Primitives
#
# Tests clock/monotonic, clock/realtime, clock/cpu,
# time/sleep, time/stopwatch, and time/elapsed.

(import-file "./examples/assertions.lisp")

# ========================================
# 1. clock/monotonic returns a number
# ========================================
(display "=== 1. clock/monotonic ===\n")

(var t1 (clock/monotonic))
(assert-true (number? t1) "clock/monotonic returns a number")
(assert-true (>= t1 0.0) "clock/monotonic is non-negative")

(var t2 (clock/monotonic))
(assert-true (>= t2 t1) "clock/monotonic is monotonically non-decreasing")
(display "  clock/monotonic works\n")

# ========================================
# 2. clock/realtime returns epoch seconds
# ========================================
(display "\n=== 2. clock/realtime ===\n")

(var epoch (clock/realtime))
(assert-true (number? epoch) "clock/realtime returns a number")
(assert-true (> epoch 1700000000.0) "clock/realtime is a plausible epoch timestamp")
(display "  clock/realtime works\n")

# ========================================
# 3. clock/cpu returns thread CPU time
# ========================================
(display "\n=== 3. clock/cpu ===\n")

(var cpu1 (clock/cpu))
(assert-true (number? cpu1) "clock/cpu returns a number")
(assert-true (>= cpu1 0.0) "clock/cpu is non-negative")

(var cpu2 (clock/cpu))
(assert-true (>= cpu2 cpu1) "clock/cpu is non-decreasing")
(display "  clock/cpu works\n")

# ========================================
# 4. time/sleep
# ========================================
(display "\n=== 4. time/sleep ===\n")

(var before (clock/monotonic))
(time/sleep 0)
(var after (clock/monotonic))
(assert-true (>= after before) "time/sleep with 0 returns immediately")
(display "  time/sleep works\n")

# ========================================
# 5. time/stopwatch
# ========================================
(display "\n=== 5. time/stopwatch ===\n")

(var sw (time/stopwatch))
(assert-true (coro? sw) "time/stopwatch returns a coroutine")

(var sample1 (coro/resume sw))
(assert-true (number? sample1) "stopwatch sample is a number")
(assert-true (>= sample1 0.0) "stopwatch sample is non-negative")

(time/sleep 0)
(var sample2 (coro/resume sw))
(assert-true (>= sample2 sample1) "stopwatch samples are non-decreasing")
(display "  time/stopwatch works\n")

# ========================================
# 6. time/elapsed
# ========================================
(display "\n=== 6. time/elapsed ===\n")

(var result (time/elapsed (fn () (+ 1 2))))
(assert-true (list? result) "time/elapsed returns a list")
(assert-eq (first result) 3 "time/elapsed preserves result value")
(assert-true (number? (first (rest result))) "time/elapsed second element is elapsed time")
(assert-true (>= (first (rest result)) 0.0) "time/elapsed time is non-negative")
(display "  time/elapsed works\n")

# ========================================
# Summary
# ========================================
(display "\n========================================\n")
(display "All time/clock tests passed!\n")
(display "========================================\n")
