; Clock and Stopwatch Benchmarks
;
; Measures the overhead of each clock primitive and the stopwatch
; coroutine. Run with: cargo run --release -- examples/benchmarks.lisp

(import-file "./examples/assertions.lisp")

; ── helpers ──────────────────────────────────────────────────────────

(var bench-iterations 1000)

(defn run-bench (label n thunk)
  (let ((t0 (clock/monotonic))
        (i 0))
    (while (< i n)
      (begin
        (thunk)
        (set i (+ i 1))))
    (let ((elapsed (- (clock/monotonic) t0)))
      (let ((ns-per (/ (* elapsed 1000000000) n)))
        (display "  ")
        (display label)
        (display ": ")
        (display ns-per)
        (display " ns/call\n")
        ns-per))))

; ── clock primitives ─────────────────────────────────────────────────

(display "=== Clock primitive overhead ===\n")

(run-bench "clock/monotonic" bench-iterations
  (fn () (clock/monotonic)))

(run-bench "clock/realtime " bench-iterations
  (fn () (clock/realtime)))

(run-bench "clock/cpu      " bench-iterations
  (fn () (clock/cpu)))

; ── clock + subtract ─────────────────────────────────────────────────

(display "\n=== Clock read + subtract ===\n")

(let ((base (clock/monotonic)))
  (run-bench "monotonic - base" bench-iterations
    (fn () (- (clock/monotonic) base))))

(let ((base (clock/cpu)))
  (run-bench "cpu - base      " bench-iterations
    (fn () (- (clock/cpu) base))))

; ── stopwatch (coroutine) ────────────────────────────────────────────

(display "\n=== Stopwatch (coro/resume) ===\n")

(let ((sw (time/stopwatch)))
  (coro/resume sw)
  (run-bench "stopwatch sample" bench-iterations
    (fn () (coro/resume sw))))

; ── time/elapsed ─────────────────────────────────────────────────────

(display "\n=== time/elapsed ===\n")

(run-bench "time/elapsed    " bench-iterations
  (fn () (time/elapsed (fn () 42))))

; ── summary ──────────────────────────────────────────────────────────

(display "\n=== Notes ===\n")
(display "  clock/monotonic uses vDSO (no syscall)\n")
(display "  clock/cpu uses clock_gettime(CLOCK_THREAD_CPUTIME_ID)\n")
(display "  stopwatch overhead is fiber context switch (~550ns)\n")
(display "  all times measured with clock/monotonic\n")
