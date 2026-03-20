(elle/epoch 1)
## Bug Regression Tests
##
## Migrated from tests/property/bugfixes.rs
## These bugs were structural, not data-dependent. Representative examples suffice.
##
## Covers:
## - StoreCapture stack mismatch (let bindings inside lambdas)
## - defn shorthand equivalence
## - List display (no `. ()` terminator)
## - or expression return value corruption in recursive calls


# ============================================================================
# Bug 1: StoreCapture stack mismatch (let bindings inside lambdas)
# ============================================================================

# let binding inside lambda preserves value
(begin
  (def f (fn (x) (let ((y x)) y)))
  (assert (= (f 42) 42) "let binding preserves positive value")
  (assert (= (f -7) -7) "let binding preserves negative value"))

# let binding with arithmetic
(begin
  (def f (fn (a b) (let ((x a) (y b)) (+ x y))))
  (assert (= (f 10 -3) 7) "let binding with arithmetic"))

# recursive function with let inside
(begin
  (def f (fn (x)
    (if (= x 0)
        (list)
        (let ((y x))
          (cons y (f (- x 1)))))))
  (assert (= (length (f 5)) 5) "recursive function with let inside"))

# append inside let inside lambda
(begin
  (def f (fn (x)
    (if (= x 0)
        (list)
        (let ((y x))
          (append (list y) (f (- x 1)))))))
  (assert (= (length (f 5)) 5) "append inside let inside lambda"))

# multiple let bindings
(begin
  (def f (fn (a b c)
    (let ((x a) (y b) (z c))
      (+ x (+ y z)))))
  (assert (= (f 1 2 3) 6) "multiple let bindings"))

# nested let bindings
(begin
  (def f (fn (a b)
    (let ((x a))
      (let ((y b))
        (+ x y)))))
  (assert (= (f 10 20) 30) "nested let bindings"))

# let with computation
(begin
  (def f (fn (x)
    (let ((y (* x 2)) (z (+ x 1)))
      (+ y z))))
  (assert (= (f 5) 16) "let with computation (y=10, z=6, result=16)"))

# ============================================================================
# Bug 2: defn shorthand equivalence
# ============================================================================

# defn ≡ def+fn
(begin
  (defn f (x) (+ x 1))
  (assert (= (f 41) 42) "defn shorthand"))

# defn multi-param
(begin
  (defn add (a b) (+ a b))
  (assert (= (add 10 -3) 7) "defn multi-param"))

# defn recursive (factorial)
(begin
  (defn fact (n)
    (if (= n 0)
        1
        (* n (fact (- n 1)))))
  (assert (= (fact 10) 3628800) "defn recursive factorial"))

# defn with let body
(begin
  (defn double (x)
    (let ((y x))
      (+ y y)))
  (assert (= (double 21) 42) "defn with let body"))

# ============================================================================
# Bug 3: List display (no `. ()` terminator)
# ============================================================================

# list display no dot terminator
(begin
  (var list-str (string (list 1 2 3)))
  (assert (not (string/contains? list-str ". ()")) "list display no dot terminator"))

# cons chain display
(begin
  (var cons-str (string (cons 1 (cons 2 (cons 3 (list))))))
  (assert (not (string/contains? cons-str ". ()")) "cons chain display"))

# list length matches
(begin
  (assert (= (length (list 1 2 3 4 5)) 5) "list length 5")
  (assert (= (length (list)) 0) "empty list length"))

# nested list display
(begin
  (var nested-str (string (list (list 1) (list 2))))
  (assert (not (string/contains? nested-str ". ()")) "nested list display"))

# append result display
(begin
  (var append-str (string (append (list 1 2) (list 3 4))))
  (assert (not (string/contains? append-str ". ()")) "append result display"))

# ============================================================================
# Bug 4: or expression corrupts return value in recursive calls
# ============================================================================

# or expression in recursive predicate
(begin
  (var check
    (fn (x remaining)
      (if (empty? remaining)
          true
          (if (or (= x 1) (= x 2))
              false
              (check x (rest remaining))))))
  (var foo
    (fn (n seen)
      (if (= n 0)
          (list)
          (if (check n seen)
              (append (list n) (foo (- n 1) (cons n seen)))
              (foo (- n 1) seen)))))
  (assert (= (length (foo 5 (list 0))) 3) "or in recursive predicate (n=5,4,3 safe)"))

# ============================================================================
# Combined: shorthand + let + list display
# ============================================================================

# defn + let + list display
(begin
  (defn make-list (x)
    (if (= x 0)
        (list)
        (let ((y x))
          (cons y (make-list (- x 1))))))
  (var result-str (string (make-list 5)))
  (assert (not (string/contains? result-str ". ()")) "defn + let + list display"))

# defn + recursive + list display
(begin
  (defn build (n)
    (if (= n 0)
        (list)
        (let ((rest-list (build (- n 1))))
          (cons n rest-list))))
  (assert (= (length (build 10)) 10) "defn + recursive + list display"))

# ============================================================================
# Bug 5: Fiber locals corrupted after yield through nested call (tail-call-to-native)
#
# When a fiber yields inside a nested function call where the callee uses a
# TailCall to a native yielding primitive, the caller's frame was not saved
# because call_inner only saved the caller when fiber.suspended was already
# Some(_). For TailCall-to-native, no SuspendedFrame is created by the callee,
# so fiber.suspended was None and the caller frame was silently discarded,
# losing all stack-based locals.
# ============================================================================

(begin
  (defn do-write (port msg)
    (stream/write port msg))

  (let ((result @[]))
    (ev/run
      (fn ()
        (let ((p (port/open "/tmp/elle_bugfix5_test" :write)))
          (do-write p "hello")
          (push result p))))
    (assert (= (type (get result 0)) :port) "fiber locals not corrupted after yield through nested tail-call-to-native")))

# ============================================================================
# Bug 6: LoadLocal out-of-bounds after fiber/resume propagates SIG_IO
#
# When a fiber body wrapped in defer/protect yields SIG_IO (mask=1 doesn't
# catch it), the signal propagated to the outer ev/spawn fiber. On resumption,
# the outer fiber's locals (pre-allocated by the Nil prolog) were not saved,
# so LoadLocal panicked with "index out of bounds".
#
# Fix: when SIG_IO propagates through handle_fiber_resume_signal, build a
# SuspendedFrame::Bytecode for the outer caller (preserving locals) and a
# SuspendedFrame::FiberResume for the sub-fiber. resume_suspended uses the
# FiberResume frame to re-enter the sub-fiber via do_fiber_resume (proper
# fiber-swap path), then passes the sub-fiber's return value to the outer frame.
# ============================================================================

(begin
  # Minimal reproduction: deep call chain with many locals, fiber inside defer
  # doing I/O (stream/write to a real port). Caused LoadLocal panic before fix.
  (defn inner-with-many-locals (port msg)
    (let ((a 1) (b 2) (c 3) (d 4) (e 5) (f 6) (g 7) (h 8)
          (i 9) (j 10) (k 11) (l 12) (m 13) (n 14) (o 15) (p 16))
      (stream/write port msg)
      (+ a b c d e f g h i j k l m n o p)))

  (let ((result @[nil]))
    (ev/run
      (fn ()
        (let ((port (port/open "/tmp/elle_bugfix6_test" :write)))
          (defer (port/close port)
            (put result 0 (inner-with-many-locals port "hello"))))))
    (assert (= (get result 0) 136) "locals not corrupted after defer body fiber propagates SIG_IO (Bug 6)")))

# ============================================================================
# Bug 7: defer + I/O inside ev/spawn uses FiberResume chain correctly
#
# When a defer body fiber does multiple I/O operations (e.g. read then write),
# SIG_IO must propagate through the outer fiber to the async scheduler on each
# yield, and the I/O result must be delivered to the sub-fiber (not the outer
# frame). Before the FiberResume fix, resume_suspended ran the sub-fiber's
# bytecode directly (without the fiber-swap path), causing the port to be
# closed prematurely by the outer defer frame.
# ============================================================================

(begin
  (let ((listener (tcp/listen "127.0.0.1" 0)))
    (let ((addr (port/path listener)))
      (let ((port-num (integer (get (string/split addr ":") 1))))
        (let ((server-got @[nil]) (client-got @[nil]))
          (ev/run
            (fn ()  # server: accept, read, write, close via defer
              (let ((conn (tcp/accept listener)))
                (defer (port/close conn)
                  (let ((data (stream/read conn 64)))
                    (put server-got 0 data)
                    (stream/write conn "pong")))))
            (fn ()  # client: connect, write, read
              (let ((c (tcp/connect "127.0.0.1" port-num)))
                (stream/write c "ping")
                (let ((resp (stream/read c 64)))
                  (put client-got 0 resp)
                  (port/close c)))))
          (port/close listener)
                    # TCP ports use binary encoding; stream/read returns bytes.
          # Convert to string for assertion.
          (assert (= (string (get server-got 0)) "ping")
            "server received data from client (Bug 7)")
          (assert (= (string (get client-got 0)) "pong")
            "client received response from server (Bug 7)"))))) )

# ============================================================================
# Bug 612: cond/match corrupt previously-evaluated arguments in variadic calls
#
# When cond or match appeared as a non-first argument to a variadic native
# function, the emitter emitted the done/merge block before the arm blocks
# (because done_label was allocated first, giving it a lower label number
# than the arm blocks, and blocks were sorted by label). The done block's
# stack state was then cleared (no predecessor had yet saved it), causing
# previously-evaluated arguments to be invisible to the Call instruction.
#
# Fix: the emitter now emits blocks in append order (the order finish_block
# was called) rather than sorted by label. Merge/done blocks are always
# appended last, so their predecessor arm blocks have already emitted their
# Jump terminators and saved the correct stack state before the done block
# is processed.
# ============================================================================

# cond as non-first arg to native variadic call
(assert (= (path/join "a" (cond (true "b"))) "a/b") "cond as second arg to path/join")

# match as non-first arg to native variadic call
(assert (= (path/join "a" (match 1 (1 "b") (_ "c"))) "a/b") "match as second arg to path/join")

# cond in second position of three-arg list
(assert (= (list 1 (cond (true 2)) 3) (list 1 2 3)) "cond as second arg in list")

# cond in third position of three-arg list
(assert (= (list 1 2 (cond (true 3))) (list 1 2 3)) "cond as third arg in list")

# match in second position of three-arg list
(assert (= (list 1 (match 1 (1 2) (_ 0)) 3) (list 1 2 3)) "match as second arg in list")

# match in third position (wildcard arm)
(assert (= (list 1 2 (match 5 (1 "nope") (_ 3))) (list 1 2 3)) "match wildcard as third arg in list")

# cond with multiple clauses, non-first arg
(assert (= (path/join "a" (cond (false "x") (true "b"))) "a/b") "cond with two clauses as second arg to path/join")

# cond as first arg still works (was never broken)
(assert (= (path/join (cond (true "a")) "b") "a/b") "cond as first arg to path/join (regression guard)")
