# Process Model - Erlang-style message passing via fibers
#
# yield IS receive. The resume value IS the message. A scheduler mediates.
#
# Protocol (process yields a command, scheduler resumes with the result):
#
#   (yield [:send pid msg])    → delivers msg, resumes with :ok
#   (yield [:recv])            → delivers next message (or parks process)
#   (yield [:self])            → resumes with the process's PID
#   (yield [:spawn closure])   → creates process, resumes with its PID
#   (yield [:link pid])        → links to pid (bidirectional), resumes with :ok
#   (yield [:unlink pid])      → removes link, resumes with :ok
#   (yield [:trap-exit bool])  → sets trap_exit flag, resumes with :ok
#   (yield [:spawn-link closure]) → spawn + link in one step, resumes with PID

(import-file "./examples/assertions.lisp")

(display "=== Process Model: Erlang-style Message Passing ===\n")


# ========================================
# Process API (used inside processes)
# ========================================

(def ! (fn (pid msg) (yield [:send pid msg])))
(def recv (fn () (yield [:recv])))
(def self (fn () (yield [:self])))
(def link (fn (pid) (yield [:link pid])))
(def unlink (fn (pid) (yield [:unlink pid])))
(def trap-exit (fn (flag) (yield [:trap-exit flag])))
(def spawn (fn (closure) (yield [:spawn closure])))
(def spawn-link (fn (closure) (yield [:spawn-link closure])))


# ========================================
# The Scheduler
# ========================================

(def make-scheduler (fn ()
  # Per-PID state (parallel arrays indexed by PID):
  (let ((fibers    @[])    # fiber value
        (mboxes    @[])    # @[@[] ...] — mailbox per PID
        (resumes   @[])    # pending resume value
        (statuses  @[])    # :alive | :dead | :error
        (links     @[])    # @[@[] ...] — linked PIDs per PID
        (trapping  @[])    # @[bool ...] — trap_exit flag per PID
        (ready     @[])    # PIDs ready to run
        (waiting   @[]))   # PIDs blocked on recv

    # ---- helpers ----

    (var sched-spawn (fn (closure)
      (let ((pid (length fibers))
            (f (fiber/new closure 3)))  # mask=3: catch yield(2) + error(1)
        (push fibers f)
        (push mboxes @[])
        (push resumes nil)
        (push statuses :alive)
        (push links @[])
        (push trapping false)
        (push ready pid)
        pid)))

    # Deliver exit signal to linked processes when pid dies
    (var notify-links (fn (dead-pid reason)
      (each linked-pid in (get links dead-pid)
        (if (= (get statuses linked-pid) :alive)
          (if (get trapping linked-pid)
            # Trapping: deliver as message
            (begin
              (push (get mboxes linked-pid) [:EXIT dead-pid reason])
              nil)
            # Not trapping: kill the linked process
            (begin
              (put statuses linked-pid :error)
              # Cascade: notify this process's links too
              (notify-links linked-pid [:linked dead-pid reason])))))))

    # Add a bidirectional link
    (var add-link (fn (a b)
      (push (get links a) b)
      (push (get links b) a)))

    # Remove a bidirectional link
    (var remove-link (fn (a b)
      (let ((a-links (get links a))
            (b-links (get links b)))
        # Remove b from a's links
        (var i 0)
        (while (< i (length a-links))
          (begin
            (if (= (get a-links i) b)
              (begin (remove a-links i))
              (set i (+ i 1)))))
        # Remove a from b's links
        (set i 0)
        (while (< i (length b-links))
          (begin
            (if (= (get b-links i) a)
              (begin (remove b-links i))
              (set i (+ i 1))))))))

    # Mark a process as dead with a reason, notify links
    (var process-exit (fn (pid reason)
      (put statuses pid :dead)
      (notify-links pid reason)))

    # Wake waiting processes that now have messages
    (var wake-waiting (fn ()
      (let ((still-waiting @[]))
        (each pid in waiting
          (if (= (get statuses pid) :alive)
            (if (> (length (get mboxes pid)) 0)
              (begin
                (let* ((mbox (get mboxes pid))
                       (msg (get mbox 0)))
                  (remove mbox 0)
                  (put resumes pid msg)
                  (push ready pid)))
              (push still-waiting pid))))
        (set waiting still-waiting))))

    # Interpret a yielded command
    (var handle-cmd (fn (pid cmd)
      (match cmd
        ([:send target-pid msg]
          (if (= (get statuses target-pid) :alive)
            (push (get mboxes target-pid) msg))
          (put resumes pid :ok)
          (push ready pid))

        ([:recv]
          (let ((mbox (get mboxes pid)))
            (if (> (length mbox) 0)
              (begin
                (let ((msg (get mbox 0)))
                  (remove mbox 0)
                  (put resumes pid msg)
                  (push ready pid)))
              (push waiting pid))))

        ([:self]
          (put resumes pid pid)
          (push ready pid))

        ([:spawn closure]
          (let ((new-pid (sched-spawn closure)))
            (put resumes pid new-pid)
            (push ready pid)))

        ([:spawn-link closure]
          (let ((new-pid (sched-spawn closure)))
            (add-link pid new-pid)
            (put resumes pid new-pid)
            (push ready pid)))

        ([:link target-pid]
          (add-link pid target-pid)
          (put resumes pid :ok)
          (push ready pid))

        ([:unlink target-pid]
          (remove-link pid target-pid)
          (put resumes pid :ok)
          (push ready pid))

        ([:trap-exit flag]
          (put trapping pid flag)
          (put resumes pid :ok)
          (push ready pid))

        (_
          (error :protocol-error "unknown scheduler command")))))

    # Run one process: resume it, handle the result
    (var run-one (fn (pid)
      (if (not (= (get statuses pid) :alive))
        nil  # skip dead/errored processes
        (let ((f (get fibers pid))
              (resume-val (get resumes pid)))
          (put resumes pid nil)
          (fiber/resume f resume-val)
          (let ((bits (fiber/bits f)))
            (cond
              # Process completed normally
              ((= (fiber/status f) :dead)
                (process-exit pid [:normal (fiber/value f)]))

              # Process errored
              ((= bits 1)
                (process-exit pid [:error (fiber/value f)]))

              # Process yielded a command
              ((= bits 2)
                (handle-cmd pid (fiber/value f)))

              (true
                (error :scheduler-error "unexpected signal bits"))))))))

    # ---- main loop ----

    (var any-alive? (fn ()
      (or (not (empty? ready)) (not (empty? waiting)))))

    (var sched-run (fn (init)
      (sched-spawn init)

      (while (any-alive?)
        (begin
          (wake-waiting)

          (if (and (empty? ready) (not (empty? waiting)))
            (error :deadlock "all processes waiting, no messages pending"))

          (let ((batch ready))
            (set ready @[])
            (each pid in batch
              (run-one pid)))))))

    sched-run)))


# ========================================
# Test 1: Ping-pong
# ========================================
(display "\n--- Test 1: Ping-Pong ---\n")

(let ((run (make-scheduler)))
  (run (fn ()
    (let ((me (self))
          (ponger (spawn (fn ()
                    (let ((msg (recv)))
                      (match msg
                        ([from :ping]
                          (! from :pong))))))))
      (! ponger [me :ping])
      (let ((reply (recv)))
        (display "  got: ")
        (display reply)
        (display "\n")
        (assert-eq reply :pong "ping-pong reply is :pong")))))
  (display "✓ Ping-pong works\n"))


# ========================================
# Test 2: Ring of processes
# ========================================
(display "\n--- Test 2: Message Ring ---\n")

(let ((run (make-scheduler)))
  (run (fn ()
    (let ((me (self)))
      (var make-forwarder (fn (next)
        (fn ()
          (let ((msg (recv)))
            (! next (+ msg 1))))))

      (let* ((p3 (spawn (make-forwarder me)))
             (p2 (spawn (make-forwarder p3)))
             (p1 (spawn (make-forwarder p2))))
        (! p1 0)
        (let ((result (recv)))
          (display "  sent 0, received ")
          (display result)
          (display " (passed through 3 forwarders)\n")
          (assert-eq result 3 "ring increments message 3 times"))))))
  (display "✓ Message ring works\n"))


# ========================================
# Test 3: Fan-in
# ========================================
(display "\n--- Test 3: Fan-in ---\n")

(let ((run (make-scheduler)))
  (run (fn ()
    (let ((me (self)))
      (var i 0)
      (while (< i 5)
        (begin
          (let ((id i))
            (spawn (fn () (! me id))))
          (set i (+ i 1))))

      (var total 0)
      (set i 0)
      (while (< i 5)
        (begin
          (set total (+ total (recv)))
          (set i (+ i 1))))

      (display "  sum of worker ids: ")
      (display total)
      (display "\n")
      (assert-eq total 10 "fan-in: 0+1+2+3+4 = 10"))))
  (display "✓ Fan-in works\n"))


# ========================================
# Test 4: Link — crash propagation
# ========================================
(display "\n--- Test 4: Link Crash Propagation ---\n")

# When a linked process crashes, the linked partner should also die.
# We test this by having a supervisor (trap_exit) observe the cascade.

(let ((run (make-scheduler)))
  (run (fn ()
    (trap-exit true)
    (let ((me (self)))

      # Spawn worker-a, link it to us
      (let ((worker-a (spawn-link (fn ()
              # worker-a spawns worker-b and links to it
              (let ((b (spawn-link (fn ()
                          # worker-b crashes
                          (fiber/signal 1 [:boom "worker-b crashed"])))))
                # worker-a waits for something (will be killed by link)
                (recv))))))

        # We should get an EXIT message because worker-a died (linked to us)
        (let ((msg (recv)))
          (display "  supervisor received: ")
          (display msg)
          (display "\n")
          (match msg
            ([:EXIT pid reason]
              (assert-eq pid worker-a "EXIT from worker-a")
              (display "  ✓ got EXIT from linked worker\n"))))))))

  (display "✓ Link crash propagation works\n"))


# ========================================
# Test 5: trap_exit — convert signals to messages
# ========================================
(display "\n--- Test 5: trap_exit ---\n")

(let ((run (make-scheduler)))
  (run (fn ()
    (trap-exit true)
    (let* ((me (self))
           (child (spawn-link (fn ()
                    (fiber/signal 1 [:intentional "test crash"])))))

      (let ((msg (recv)))
        (display "  trapped: ")
        (display msg)
        (display "\n")
        (match msg
          ([:EXIT pid reason]
            (assert-eq pid child "EXIT from child")
            (match reason
              ([:error _]
                (assert-true true "got error reason")))))))))

  (display "✓ trap_exit works\n"))


# ========================================
# Test 6: Normal exit delivers [:EXIT pid [:normal val]]
# ========================================
(display "\n--- Test 6: Normal Exit Notification ---\n")

(let ((run (make-scheduler)))
  (run (fn ()
    (trap-exit true)
    (let ((child (spawn-link (fn () 42))))
      (let ((msg (recv)))
        (display "  normal exit: ")
        (display msg)
        (display "\n")
        (match msg
          ([:EXIT pid reason]
            (assert-eq pid child "EXIT from child")
            (match reason
              ([:normal val]
                (assert-eq val 42 "normal exit value is 42")))))))))

  (display "✓ Normal exit notification works\n"))


# ========================================
# Test 7: Unlink prevents notification
# ========================================
(display "\n--- Test 7: Unlink ---\n")

(let ((run (make-scheduler)))
  (run (fn ()
    (trap-exit true)
    (let* ((me (self))
           (child (spawn-link (fn ()
                    # Wait for go signal, then crash
                    (recv)
                    (fiber/signal 1 [:boom "crash"])))))

      # Unlink before the child crashes
      (unlink child)

      # Tell child to proceed
      (! child :go)

      # Send ourselves a marker to prove we don't get EXIT
      (! me :still-alive)

      (let ((msg (recv)))
        (display "  after unlink, got: ")
        (display msg)
        (display "\n")
        (assert-eq msg :still-alive "no EXIT after unlink")))))

  (display "✓ Unlink works\n"))


(display "\n=== All process model tests passed ===\n")
