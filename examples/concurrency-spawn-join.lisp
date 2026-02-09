#!/usr/bin/env elle
;;; Concurrency with spawn and join
;;;
;;; This example demonstrates the spawn and join primitives for concurrent
;;; execution in Elle. The spawn primitive creates a new thread that executes
;;; a closure, and join waits for the thread to complete and returns its result.
;;;
;;; Key concepts:
;;; 1. spawn takes a closure and executes it in a new thread
;;; 2. The closure can capture immutable values from the parent scope
;;; 3. join blocks until the thread completes and returns the result
;;; 4. Closures that capture mutable values (tables) cannot be spawned

(display "=== Elle Concurrency Example: spawn and join ===")
(newline)

;;; Example 1: Simple computation in a thread
(display "Example 1: Simple computation in a thread")
(newline)

(let ((x 10) (y 20))
  (let ((handle (spawn (lambda () (+ x y)))))
    (let ((result (join handle)))
      (display "Result of (+ 10 20) in thread: ")
      (display result)
      (newline))))

;;; Example 2: Multiple threads computing in parallel
(display "Example 2: Multiple threads computing in parallel")
(newline)

(let ((h1 (spawn (lambda () (* 2 3))))
      (h2 (spawn (lambda () (* 4 5))))
      (h3 (spawn (lambda () (* 6 7)))))
  (let ((r1 (join h1))
        (r2 (join h2))
        (r3 (join h3)))
    (display "Results: ")
    (display r1)
    (display ", ")
    (display r2)
    (display ", ")
    (display r3)
    (newline)))

;;; Example 3: Capturing immutable values
(display "Example 3: Capturing immutable values")
(newline)

(let ((name "Alice")
      (age 30))
  (let ((handle (spawn (lambda () 
                         (string-append "Hello, " name "! You are " (string age) " years old.")))))
    (display (join handle))
    (newline)))

;;; Example 4: Capturing vectors
(display "Example 4: Capturing vectors")
(newline)

(let ((numbers [1 2 3 4 5]))
  (let ((handle (spawn (lambda () 
                         (let ((sum (+ 1 (+ 2 (+ 3 (+ 4 5))))))
                           sum)))))
    (display "Sum of [1 2 3 4 5]: ")
    (display (join handle))
    (newline)))

;;; Example 5: Closure with conditional logic
(display "Example 5: Closure with conditional logic")
(newline)

(let ((threshold 50))
  (let ((handle (spawn (lambda () 
                         (if (> threshold 40)
                             "threshold is high"
                             "threshold is low")))))
    (display (join handle))
    (newline)))

;;; Example 6: Closure returning a value
(display "Example 6: Closure returning a value")
(newline)

(let ((multiplier 3))
  (let ((handle (spawn (lambda () 
                         (* 7 multiplier)))))
    (display "7 * 3 = ")
    (display (join handle))
    (newline)))

;;; Example 7: Spawning closures with different capture patterns
(display "Example 7: Spawning closures with different capture patterns")
(newline)

(let ((x 100))
  (let ((h1 (spawn (lambda () x)))
        (h2 (spawn (lambda () (+ x 50)))))
    (display "Captured value: ")
    (display (join h1))
    (newline)
    (display "Computed value: ")
    (display (join h2))
    (newline)))

;;; Example 8: Using sleep with threads
(display "Example 8: Using sleep with threads")
(newline)

(let ((handle (spawn (lambda () 
                       (begin
                         (display "Thread started")
                         (newline)
                         (sleep 0.1)
                         (display "Thread finished after sleep")
                         (newline)
                         42)))))
  (display "Main thread waiting for spawned thread...")
  (newline)
  (let ((result (join handle)))
    (display "Spawned thread returned: ")
    (display result)
    (newline)))

;;; Example 9: Current thread ID
(display "Example 9: Current thread ID")
(newline)

(display "Main thread ID: ")
(display (current-thread-id))
(newline)

(let ((handle (spawn (lambda () (current-thread-id)))))
  (display "Spawned thread ID: ")
  (display (join handle))
  (newline))

;;; Example 10: Complex computation with multiple captures
(display "Example 10: Complex computation with multiple captures")
(newline)

(let ((a 2) (b 3) (c 4) (d 5))
  (let ((handle (spawn (lambda () 
                         (+ (* a b) (* c d))))))
    (display "Result of (+ (* 2 3) (* 4 5)): ")
    (display (join handle))
    (newline)))

(display "=== End of concurrency example ===")
(newline)
