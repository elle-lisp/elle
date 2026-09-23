(elle/epoch 12)
# audited: 2026-09-23
# The process scheduler: runs process fibers in rounds, forwards their I/O, and tears down orphans.
# lib/process/overview.md
# docs/processes.md
# docs/process-scheduler.md
#
# Signal mask: |:yield :error :fuel :io :exec :wait| — the scheduler catches
# all six. :exec is a capability bit for subprocess operations. :wait carries
# structured concurrency (ev/spawn, ev/join, ev/select) inside processes.

(def make-core (import "std/process/core"))
(def make-waits (import "std/process/waits"))
(def make-commands (import "std/process/commands"))

# The clock advances this many ticks per second the scheduler waits on I/O.
(def io-ticks-per-second 1000.0)

(defn sleep-request [seconds]
  "The I/O request that (ev/sleep seconds) yields, taken without yielding it."
  (let [f (fiber/new (fn [] (ev/sleep seconds)) |:io|)]
    (fiber/resume f)
    (fiber/value f)))

(defn make-scheduler [&named fuel]
  "A process scheduler whose processes run :fuel instructions a turn (1000 by
   default). It builds no I/O backend: it forwards each I/O request to the
   parent scheduler, the one that runs the code which calls run or start."
  (let* [core (make-core (or fuel 1000))
         waits (make-waits core)
         handle-cmd (make-commands core)
         quantum core:quantum
         ready core:ready
         waiting core:waiting
         io-pending core:io-pending
         io-completions waits:io-completions
         io-wakeup-box waits:io-wakeup-box
         fuel-preempted @[]]
    (defn set-running [pid]
      (rebox waits:spawning-pid pid))

    (defn has-work? []
      (or (not (empty? ready)) (not (empty? waiting)) (> (length io-pending) 0)
          (waits:has-sub-work?)))

    # ---- signal dispatch (shared by run-one and complete-io) ----

    (defn dispatch-signal [pid f]
      (let [bits (fiber/bits f)]
        (cond
          (= (fiber/status f) :dead)
            (core:process-exit pid [:normal (fiber/value f)])

          # Error
          (not (= 0 (bit/and bits 1)))
            (core:process-exit pid [:error (fiber/value f)])

          # Fuel exhaustion — re-queue for next round, mark fuel-only
          (not (= 0 (bit/and bits 4096))) (begin
            (push ready pid)
            (push fuel-preempted pid))

          # I/O — forward to the parent scheduler, park process
          (not (= 0 (bit/and bits 512)))
            (let [id (waits:forward-io (fiber/value f))]
              (if (and (array? id) (= (first id) :error))
                (begin
                  (fiber/abort f (get id 1))
                  (dispatch-signal pid f))
                (put io-pending id @{:pid pid})))

          # Wait — structured concurrency (ev/join, ev/select, ev/abort)
          (not (= 0 (bit/and bits 16384)))
            (let [request (fiber/value f)]
              (if (waits:known-op? (get request :op))
                (waits:handle-wait pid request)
                (begin
                  (fiber/abort f (waits:unknown-op (get request :op)))
                  (dispatch-signal pid f))))

          # Yield — scheduler command
          (not (= 0 (bit/and bits 2))) (handle-cmd pid (fiber/value f))
          (error {:error :scheduler-error :message "unexpected signal bits"}))))

    (defn run-one [pid]
      (when (core:alive? pid)
        (let* [p (core:proc-get pid)
               f (get p :fiber)
               resume-val (get p :resume)]
          (put p :resume nil)
          (set-running pid)
          (fiber/set-fuel f quantum)
          (fiber/resume f resume-val)
          (dispatch-signal pid f))))

    # ---- I/O completion handling ----

    (defn complete-io [completions]
      "Resume or abort the process or sub-fiber behind each completion. A
       completion relayed for a nested scheduler goes to its queue, and the
       bump to its wake-box wakes it."
      (each completion in completions
        (let* [id (get completion :id)
               entry (get io-pending id)
               err (get completion :error)]
          (del io-pending id)
          (when (not (nil? entry))
            (let [pid (get entry :pid)
                  sub-fiber (get entry :fiber)
                  queue (get entry :queue)]
              (cond
                # The alarm's completion only ends the wait in wait-for-io.
                (get entry :alarm) nil
                (not (nil? queue))
                  (begin
                    (push queue completion)
                    (rebox entry:wake-box (+ (unbox entry:wake-box) 1)))
                (not (nil? sub-fiber))
                  (when (= (fiber/status sub-fiber) :paused)
                    (if (nil? err)
                      (fiber/resume sub-fiber (get completion :value))
                      (fiber/abort sub-fiber err))
                    (waits:after-resume sub-fiber pid))
                (when (core:alive? pid)
                  (let [f (get (core:proc-get pid) :fiber)]
                    (set-running pid)
                    (fiber/set-fuel f quantum)
                    (if (nil? err)
                      (fiber/resume f (get completion :value))
                      (fiber/abort f err))
                    (dispatch-signal pid f)))))))))

    (defn reap-io []
      "Handle the completions the parent scheduler has forwarded so far."
      (when (> (length io-completions) 0)
        (let [batch (->list io-completions)]
          (core:refill io-completions [])
          (complete-io batch))))

    # Program-completion teardown, mirroring ev/run's make-async-scheduler.
    # Once no process is alive, every remaining sub-fiber is an orphan: no
    # live process can wake a parked futex, deliver its I/O, or observe its
    # result. Left alone they keep has-work? true forever, so sched-run
    # spins (futex orphan) or blocks on io/wait (I/O orphan). Abort each so
    # its defer/protect cleanup runs, cancel its forwarded I/O, and empty
    # the sub-fiber state so has-work? goes false and sched-run returns.
    #
    # Aborting runs defers, which may submit fresh I/O or re-park on a
    # futex; those land back in the tracked collections and are caught the
    # next round. Bounded so a defer that stubbornly re-parks cannot bring
    # the hang back; a final clear-sub-state drops anything still lingering
    # past the round bound.
    (defn abort-orphan-subs []
      (def @rounds 0)
      (while (and (< rounds 64) (has-work?))
        (assign rounds (+ rounds 1))
        (let [victims (waits:collect-orphan-subs)]
          (waits:clear-sub-state)
          (each v in victims
            (let [f (get v :fiber)]
              (when (= (fiber/status f) :paused)
                (protect (fiber/abort f {:error :shutdown}))
                (waits:after-resume f (or (get v :pid) 0)))))
          (reap-io)))
      (waits:clear-sub-state))

    (defn stall []
      "Every waiting process waits on nothing that can come. While PID 0 is
       one of them the program cannot end; once PID 0 has, they are idle for good."
      (if (core:alive? 0)
        (error {:error :deadlock
                :message "all processes waiting, no messages pending"})
        (core:shutdown-idle)))

    (defn arm-alarm [due]
      "Forward a sleep that ends when the clock reaches tick due, and return
       its id. Raises when the parent refuses it."
      (let* [seconds (/ (max 0 (- due (core:now))) io-ticks-per-second)
             id (waits:forward-io (sleep-request seconds))]
        (when (and (array? id) (= (first id) :error)) (error (get id 1)))
        (put io-pending id @{:alarm true})
        id))

    (defn completed? [id]
      "Whether the parent has delivered the completion of submission id."
      (not (nil? (find (fn [c] (= (get c :id) id)) (->list io-completions)))))

    (defn wait-for-io [expected]
      "Park until the parent delivers a completion or the earliest timer falls
       due. The clock advances one tick per whole millisecond of the park, and
       at least to that timer when the park ran to it. It advances before any
       process runs, so a process resumed by the completion reads the new tick."
      (let* [started (clock/monotonic)
             due (when (not (empty? core:timers)) (core:earliest-timer))
             alarm (when due (arm-alarm due))]
        (ev/futex-wait :io-forward-wakeup io-wakeup-box expected)
        (core:advance-to (+ (core:now)
                            (floor (* (- (clock/monotonic) started)
                                      io-ticks-per-second))))
        (when (and alarm (completed? alarm))
          (core:advance-to (max (core:now) due)))
        (reap-io)
        (when alarm (core:cancel-io alarm))))

    (defn idle []
      "No process is ready: wait for I/O or the next timer, jump to the next
       timer, or stall."
      (cond
        (not (has-work?)) nil

        # I/O in flight — park until a completion or the next timer
        (> (length io-pending) 0)
          (begin
            (let [expected (unbox io-wakeup-box)]
              (reap-io)
              (when (empty? ready) (wait-for-io expected)))
            (waits:drain-sub-runnable))

        # Waiting with timers and no I/O — fast-forward the clock
        (not (empty? core:timers))
          (begin
            (core:advance-to (core:earliest-timer))
            (core:fire-timers)
            (core:wake-waiting)
            (when (and (empty? ready) (not (empty? waiting))
                       (empty? core:timers))
              (stall)))

        # Waiting with no timers, no I/O
        (not (empty? waiting)) (stall)))

    (defn sched-run [init]
      # Parameterize *spawn* so that ev/spawn inside any process or
      # sub-fiber registers fibers with THIS scheduler. Spawn the init
      # process inside the parameterize too — fibers capture their parameter
      # environment at creation time.
      (parameterize ((*spawn* waits:spawn-fn))
        (core:spawn init)
        (while (has-work?)
          (core:tick)
          (core:fire-timers)
          (reap-io)
          # Skip draining sub-fibers when ALL ready processes are just
          # refueling. Pumping sub-fibers (h2 reader, server connection)
          # during fuel preemption races with process-level h2 setup and
          # causes protocol errors on shared TCP connections.
          (let [fuel-only (and (not (empty? ready))
                               (= (length ready) (length fuel-preempted)))]
            (core:refill fuel-preempted [])
            (unless fuel-only (waits:drain-sub-runnable)))
          (waits:wake-futex-ready)
          (core:wake-waiting)

          # No process left alive: every remaining sub-fiber is an orphan.
          # Tear them down here — after draining, so a fire-and-forget
          # sub-fiber still runs once, but before the idle handler below
          # would spin on a futex orphan or block on io/wait for an I/O
          # orphan that never completes.
          (when (not (core:any-alive?)) (abort-orphan-subs))
          (when (empty? ready) (idle))

          (let [batch (->list ready)]
            (core:refill ready [])
            (each pid in batch
              (run-one pid)))

          # Give the parent scheduler a turn when forwarded I/O is pending
          # and every ready process is merely refueling. The parent can only
          # deliver a completion while this scheduler is suspended, and a
          # process that computes without pause keeps `ready` non-empty
          # forever, so the idle handler above never runs.
          #
          # The turn is a zero-length sleep, not a park on the completion
          # futex: these processes are ready, and a forwarded completion
          # can DEPEND on one of them running. An h2 client sub-fiber
          # parked in `read` waits for the request its own process has
          # not finished sending, so a park here waits on work only the
          # parked scheduler can do (tests/elle/process-io-park.lisp,
          # tests/elle/h2-headers-in-process.lisp). The sleep suspends
          # long enough for the parent to pump, then returns whether a
          # completion arrived or not.
          (when (and (> (length io-pending) 0) (= (length io-completions) 0)
                     (not (empty? ready))
                     (= (length ready) (length fuel-preempted)))
            (ev/sleep 0)
            (reap-io)))

        # Propagate PID 0 errors to caller
        (let [err (get (core:proc-get 0) :exit-reason)]
          (when err (error err)))))

    (defn sched-inject [pid msg]
      (when (< pid (length core:procs)) (core:deliver pid msg)))

    (defn sched-process-info [pid]
      (when (< pid (length core:procs))
        (let [p (core:proc-get pid)]
          {:pid (get p :pid)
           :status (get p :status)
           :name (get p :name)
           :mbox-size (length (get p :mbox))
           :links (get p :links)
           :trapping (get p :trapping)})))

    {:run sched-run
     :spawn core:spawn
     :inject sched-inject
     :process-info sched-process-info}))

(defn run [sched init]
  "Run init-closure on an existing scheduler. Use this when you need to
   share a scheduler across multiple entry points or configure it separately.
   See also: start (which creates a scheduler for you)."
  ((get sched :run) init))

(defn start [init &named fuel]
  "Create a fresh scheduler and run init-closure as the first process.
   Blocks until no process can run again. Returns the scheduler, or raises
   {:error :process-error} when the first process dies of an error, a link,
   or an exit another process sent it.
   This is the primary entry point for most programs. Use `run` instead
   when you need to pre-configure or reuse a scheduler."
  (let [sched (make-scheduler :fuel fuel)]
    ((get sched :run) init)
    sched))

(defn process-info [sched pid]
  "Query process state from outside."
  ((get sched :process-info) pid))

(defn inject [sched pid msg]
  "Send a message from outside the scheduler."
  ((get sched :inject) pid msg))

(fn []
  {:make-scheduler make-scheduler
   :run run
   :start start
   :process-info process-info
   :inject inject})
