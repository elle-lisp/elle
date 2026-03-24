## lib/process.lisp — Erlang-inspired process module
##
## Loaded via: (def process ((import-file "lib/process.lisp")))
## Usage:      (process:start (fn () (send (self) :hello) (recv)))
##
## Processes are fibers driven by a cooperative scheduler with fuel-based
## preemption. Each process has a PID, mailbox, links, monitors, optional
## name, and a process dictionary. Communication is via yield commands
## interpreted by the scheduler.
##
## Signal mask: |:yield :error :fuel :io :exec :wait| — the scheduler
## catches all six. :exec is a capability bit for subprocess operations.
## :wait enables structured concurrency (ev/spawn, ev/join, ev/select, etc.)
## inside processes.

# ============================================================================
# Process API — used inside processes (yield-based)
# ============================================================================

(defn send [pid msg]         (yield [:send pid msg]))
(defn recv []                (yield [:recv]))
(defn recv-match [pred]      (yield [:recv-match pred]))
(defn recv-timeout [ticks]   (yield [:recv-timeout ticks]))
(defn self []                (yield [:self]))
(defn spawn [closure]        (yield [:spawn closure]))
(defn spawn-link [closure]   (yield [:spawn-link closure]))
(defn spawn-monitor [closure] (yield [:spawn-monitor closure]))
(defn link [pid]             (yield [:link pid]))
(defn unlink [pid]           (yield [:unlink pid]))
(defn monitor [pid]          (yield [:monitor pid]))
(defn demonitor [ref]        (yield [:demonitor ref]))
(defn trap-exit [flag]       (yield [:trap-exit flag]))
(defn exit [pid reason]      (yield [:exit pid reason]))
(defn register [name]        (yield [:register name]))
(defn unregister [name]      (yield [:unregister name]))
(defn whereis [name]         (yield [:whereis name]))
(defn send-named [name msg]  (yield [:send-named name msg]))
(defn send-after [ticks pid msg] (yield [:send-after ticks pid msg]))
(defn cancel-timer [ref]     (yield [:cancel-timer ref]))
(defn put-dict [key val]     (yield [:put-dict key val]))
(defn get-dict [key]         (yield [:get-dict key]))
(defn erase-dict [key]       (yield [:erase-dict key]))


# ============================================================================
# Fresh-ref generator
# ============================================================================

(defn make-ref-gen []
  (var counter 0)
  (fn []
    (let ([r counter])
      (assign counter (+ counter 1))
      r)))


# ============================================================================
# Scheduler
# ============================================================================

(defn make-scheduler [&named fuel backend]
  (let ([quantum    (or fuel 1000)]
        [procs      @[]]       # @array of @struct (per-process state)
        [ready      @[]]       # PIDs ready to run
        [waiting    @[]]       # PIDs blocked on recv
        [io-pending @{}]       # submission-id → {:pid pid} or {:fiber fiber :pid pid}
        [names      @{}]       # name → pid
        [timers     @[]]       # @array of @struct {:ref :fire-at :pid :msg}
        [tick       (box 0)]   # logical tick (boxed for mutation in closures)
        [gen-ref    (make-ref-gen)]
        [backend    (or backend (io/backend :async))]
        # ---- structured concurrency state ----
        [sub-runnable  @[]]    # @array of @struct {:fiber :pid} — child fibers to resume
        [sub-completed @{}]    # fiber → :ok | :error
        [join-waiting  @{}]    # target-fiber → @[pid ...] — who's joining
        [select-sets   @{}])   # pid → @{:candidates @[fiber...] :woken false}

    # ---- process creation ----

    (var sched-spawn (fn [closure]
      (let ([pid   (length procs)]
            [fiber (fiber/new closure |:yield :error :fuel :io :exec :wait|)])
        (push procs @{:pid pid :fiber fiber :mbox @[] :resume nil
                      :status :alive :links @||
                      :monitors @{} :monitored-by @{}
                      :trapping false :name nil :dict @{}
                      :save-queue @[] :recv-pred nil})
        (push ready pid)
        pid)))

    # ---- helpers ----

    (var proc-get (fn [pid] (get procs pid)))

    (var alive? (fn [pid]
      (= (get (proc-get pid) :status) :alive)))

    (var deliver (fn [pid msg]
      (when (alive? pid)
        (push (get (proc-get pid) :mbox) msg))))

    # Generate a unique ref
    (var fresh-ref (fn [] (gen-ref)))

    # ---- links & monitors ----

    (var add-link (fn [a b]
      (let ([la (get (proc-get a) :links)]
            [lb (get (proc-get b) :links)])
        (put la b)
        (put lb a))))

    (var remove-link (fn [a b]
      (let ([la (get (proc-get a) :links)]
            [lb (get (proc-get b) :links)])
        (del la b)
        (del lb a))))

    (var add-monitor (fn [watcher target]
      (let ([ref (fresh-ref)]
            [wp (proc-get watcher)]
            [tp (proc-get target)])
        (put (get wp :monitors) ref target)
        (put (get tp :monitored-by) ref watcher)
        ref)))

    (var remove-monitor (fn [ref watcher-pid]
      (let ([wp (proc-get watcher-pid)])
        (when (has? (get wp :monitors) ref)
          (let ([target (get (get wp :monitors) ref)])
            (del (get wp :monitors) ref)
            (when (alive? target)
              (del (get (proc-get target) :monitored-by) ref)))))))

    # ---- exit propagation ----

    (var notify-links nil)
    (assign notify-links (fn [dead-pid reason]
      (each linked-pid in (get (proc-get dead-pid) :links)
        (when (alive? linked-pid)
          (if (get (proc-get linked-pid) :trapping)
            (deliver linked-pid [:EXIT dead-pid reason])
            (begin
              (put (proc-get linked-pid) :status :dead)
              (notify-links linked-pid [:linked dead-pid reason])))))))

    (var notify-monitors (fn [dead-pid reason]
      (each ref in (keys (get (proc-get dead-pid) :monitored-by))
        (let ([watcher (get (get (proc-get dead-pid) :monitored-by) ref)])
          (when (alive? watcher)
            (deliver watcher [:DOWN ref dead-pid reason])
            (del (get (proc-get watcher) :monitors) ref))))))

    (var unregister-name (fn [pid]
      (let* ([p (proc-get pid)]
             [n (get p :name)])
        (when n
          (del names n)
          (put p :name nil)))))

    # Cancel any in-flight I/O for a process (includes sub-fiber I/O)
    (var cancel-process-io (fn [pid]
      (var to-cancel @[])
      (each [id entry] in (pairs io-pending)
        (when (= (get entry :pid) pid)
          (push to-cancel id)))
      (each id in to-cancel
        (del io-pending id)
        (io/cancel backend id))))

    (var process-exit (fn [pid reason]
      (put (proc-get pid) :status :dead)
      (cancel-process-io pid)
      (unregister-name pid)
      (notify-links pid reason)
      (notify-monitors pid reason)))

    # ---- timers ----

    (var fire-timers (fn []
      (let ([still @[]]
            [now (unbox tick)])
        (each timer in timers
          (if (>= now (get timer :fire-at))
            (deliver (get timer :pid) (get timer :msg))
            (push still timer)))
        (assign timers still))))

    # ---- selective receive helpers ----

    # Scan mailbox for a message matching pred, moving non-matches to save-queue
    (var scan-mbox (fn [pid pred]
      (let* ([p (proc-get pid)]
             [mbox (get p :mbox)]
             [save (get p :save-queue)])
        (var found nil)
        (while (and (nil? found) (> (length mbox) 0))
          (let ([msg (get mbox 0)])
            (remove mbox 0)
            (if (pred msg)
              (assign found msg)
              (push save msg))))
        found)))

    # Restore save-queue to front of mailbox
    (var restore-save-queue (fn [pid]
      (let* ([p (proc-get pid)]
             [save (get p :save-queue)]
             [mbox (get p :mbox)])
        # Prepend save-queue items back to mbox (in order)
        (var i (- (length save) 1))
        (while (>= i 0)
          # Insert at front of mbox
          (let ([tmp @[]])
            (push tmp (get save i))
            (each m in mbox (push tmp m))
            # Replace mbox contents
            (while (> (length mbox) 0) (pop mbox))
            (each m in tmp (push mbox m)))
          (assign i (- i 1)))
        # Clear save queue
        (while (> (length save) 0) (pop save))
        (put p :recv-pred nil))))

    # ---- waking ----

    (var wake-waiting (fn []
      (let ([still @[]])
        (each pid in waiting
          (if (not (alive? pid))
            nil  # skip dead
            (let* ([p (proc-get pid)]
                   [mbox (get p :mbox)]
                   [pred (get p :recv-pred)])
              (if (nil? pred)
                # Plain recv — take first message
                (if (> (length mbox) 0)
                  (begin
                    (let ([msg (get mbox 0)])
                      (remove mbox 0)
                      (put p :resume msg)
                      (push ready pid)))
                  (push still pid))
                # Selective recv — scan for matching message
                (let ([found (scan-mbox pid pred)])
                  (if (not (nil? found))
                    (begin
                      (restore-save-queue pid)
                      (put p :resume found)
                      (push ready pid))
                    (push still pid)))))))
        (assign waiting still))))

    # ---- structured concurrency: sub-fiber completion ----

    (var complete-sub-fiber (fn [fiber status]
      "Record sub-fiber completion, wake join and select waiters."
      (put sub-completed fiber status)

      # Wake join waiters
      (let ([waiters (get join-waiting fiber)])
        (when (not (nil? waiters))
          (del join-waiting fiber)
          (let ([pair [(= status :ok) (fiber/value fiber)]])
            (each pid in waiters
              (when (alive? pid)
                (put (proc-get pid) :resume pair)
                (push ready pid))))))

      # Wake select waiters
      (each [pid entry] in (pairs select-sets)
        (when (not (get entry :woken))
          (when (not (nil? (find (fn [f] (= f fiber)) (get entry :candidates))))
            (put entry :woken true)
            (del select-sets pid)
            (when (alive? pid)
              (put (proc-get pid) :resume fiber)
              (push ready pid)))))))

    (var handle-sub-fiber-after-resume nil)
    (assign handle-sub-fiber-after-resume (fn [fiber pid]
      "Route a sub-fiber after resume — same logic as ev/run's handle-fiber-after-resume."
      (case (fiber/status fiber)
        :dead   (complete-sub-fiber fiber :ok)
        :error  (complete-sub-fiber fiber :error)
        :paused (let ([bits (fiber/bits fiber)])
                  (cond
                    ((not (= 0 (bit/and bits 1)))       # SIG_ERROR
                      (complete-sub-fiber fiber :error))
                    ((not (= 0 (bit/and bits 512)))     # SIG_IO
                      (let ([[ok? result] (protect (io/submit backend (fiber/value fiber)))])
                        (if ok?
                          (put io-pending result @{:fiber fiber :pid pid})
                          (begin
                            (fiber/abort fiber result)
                            (handle-sub-fiber-after-resume fiber pid)))))
                    (true
                      (push sub-runnable @{:fiber fiber :pid pid})))))))

    # Drain sub-fiber runnable queue
    (var drain-sub-runnable (fn []
      (while (> (length sub-runnable) 0)
        (let* ([entry (pop sub-runnable)]
               [fiber (get entry :fiber)]
               [pid   (get entry :pid)]
               [status (fiber/status fiber)])
          (cond
            ((= status :dead)  (complete-sub-fiber fiber :ok))
            ((= status :error) (complete-sub-fiber fiber :error))
            (true (begin (fiber/resume fiber)
                         (handle-sub-fiber-after-resume fiber pid))))))))

    # ---- structured concurrency: wait dispatch ----

    (var handle-wait (fn [pid request]
      (case (get request :op)
        :join
          (let ([target (get request :fiber)])
            (let ([comp (get sub-completed target)])
              (if (not (nil? comp))
                # Already completed
                (begin
                  (put (proc-get pid) :resume [(= comp :ok) (fiber/value target)])
                  (push ready pid))
                # Check raw fiber status
                (let ([status (fiber/status target)])
                  (cond
                    ((= status :dead)
                      (put (proc-get pid) :resume [true (fiber/value target)])
                      (push ready pid))
                    ((= status :error)
                      (put (proc-get pid) :resume [false (fiber/value target)])
                      (push ready pid))
                    (true
                      (let ([ws (or (get join-waiting target)
                                    (let ([w @[]]) (put join-waiting target w) w))])
                        (push ws pid)
                        # Ensure the target fiber gets pumped
                        (when (nil? (get sub-completed target))
                          (push sub-runnable @{:fiber target :pid pid})))))))))

        :select
          (let ([candidates (get request :fibers)])
            # Check if any candidate already completed
            (let ([done (find (fn [f] (or (not (nil? (get sub-completed f)))
                                          (= (fiber/status f) :dead)
                                          (= (fiber/status f) :error)))
                              candidates)])
              (if (not (nil? done))
                (begin
                  (put (proc-get pid) :resume done)
                  (push ready pid))
                (begin
                  (put select-sets pid @{:candidates candidates :woken false})
                  # Ensure all candidates get pumped
                  (each f in candidates
                    (when (nil? (get sub-completed f))
                      (push sub-runnable @{:fiber f :pid pid})))))))

        :abort
          (let ([target (get request :fiber)])
            (let ([comp (get sub-completed target)])
              (if (not (nil? comp))
                # Already completed — no-op
                (begin
                  (put (proc-get pid) :resume nil)
                  (push ready pid))
                (begin
                  (protect (fiber/abort target {:error :aborted}))
                  (handle-sub-fiber-after-resume target pid)
                  (put (proc-get pid) :resume nil)
                  (push ready pid)))))

        (error {:error :protocol-error
                :message (string "unknown :wait op: " (get request :op))}))))

    # ---- command dispatch ----

    (var handle-cmd (fn [pid cmd]
      (let ([tag (get cmd 0)])
        (case tag

          :send
            (let ([target (get cmd 1)]
                  [msg    (get cmd 2)])
              (deliver target msg)
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :recv
            (let* ([p (proc-get pid)]
                   [mbox (get p :mbox)])
              (if (> (length mbox) 0)
                (begin
                  (let ([msg (get mbox 0)])
                    (remove mbox 0)
                    (put p :resume msg)
                    (push ready pid)))
                (push waiting pid)))

          :recv-match
            (let* ([pred (get cmd 1)]
                   [p (proc-get pid)]
                   [found (scan-mbox pid pred)])
              (if (not (nil? found))
                (begin
                  (restore-save-queue pid)
                  (put p :resume found)
                  (push ready pid))
                (begin
                  (put p :recv-pred pred)
                  (push waiting pid))))

          :recv-timeout
            (let* ([ticks (get cmd 1)]
                   [p (proc-get pid)]
                   [mbox (get p :mbox)])
              (if (> (length mbox) 0)
                (begin
                  (let ([msg (get mbox 0)])
                    (remove mbox 0)
                    (put p :resume msg)
                    (push ready pid)))
                (let ([ref (fresh-ref)]
                      [fire-at (+ (unbox tick) ticks)])
                  (push timers @{:ref ref :fire-at fire-at
                                 :pid pid :msg :timeout})
                  (push waiting pid))))

          :self
            (begin
              (put (proc-get pid) :resume pid)
              (push ready pid))

          :spawn
            (let ([new-pid (sched-spawn (get cmd 1))])
              (put (proc-get pid) :resume new-pid)
              (push ready pid))

          :spawn-link
            (let ([new-pid (sched-spawn (get cmd 1))])
              (add-link pid new-pid)
              (put (proc-get pid) :resume new-pid)
              (push ready pid))

          :spawn-monitor
            (let* ([new-pid (sched-spawn (get cmd 1))]
                   [ref (add-monitor pid new-pid)])
              (put (proc-get pid) :resume [new-pid ref])
              (push ready pid))

          :link
            (begin
              (add-link pid (get cmd 1))
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :unlink
            (begin
              (remove-link pid (get cmd 1))
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :monitor
            (let ([ref (add-monitor pid (get cmd 1))])
              (put (proc-get pid) :resume ref)
              (push ready pid))

          :demonitor
            (begin
              (remove-monitor (get cmd 1) pid)
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :trap-exit
            (begin
              (put (proc-get pid) :trapping (get cmd 1))
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :exit
            (let ([target (get cmd 1)]
                  [reason (get cmd 2)])
              (if (= target pid)
                # Self-exit
                (process-exit pid reason)
                # Kill another process
                (when (alive? target)
                  (if (and (get (proc-get target) :trapping)
                           (not (= reason :kill)))
                    (deliver target [:EXIT pid reason])
                    (process-exit target [:killed reason]))))
              (when (alive? pid)
                (put (proc-get pid) :resume :ok)
                (push ready pid)))

          :register
            (let ([name (get cmd 1)])
              (put names name pid)
              (put (proc-get pid) :name name)
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :unregister
            (let ([name (get cmd 1)])
              (del names name)
              (put (proc-get pid) :name nil)
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :whereis
            (let ([name (get cmd 1)])
              (put (proc-get pid) :resume (get names name nil))
              (push ready pid))

          :send-named
            (let* ([name (get cmd 1)]
                   [msg  (get cmd 2)]
                   [target (get names name nil)])
              (when target (deliver target msg))
              (put (proc-get pid) :resume :ok)
              (push ready pid))

          :send-after
            (let* ([ticks  (get cmd 1)]
                   [target (get cmd 2)]
                   [msg    (get cmd 3)]
                   [ref    (fresh-ref)]
                   [fire-at (+ (unbox tick) ticks)])
              (push timers @{:ref ref :fire-at fire-at
                             :pid target :msg msg})
              (put (proc-get pid) :resume ref)
              (push ready pid))

          :cancel-timer
            (let ([ref (get cmd 1)]
                  [found false]
                  [still @[]])
              (each timer in timers
                (if (= (get timer :ref) ref)
                  (assign found true)
                  (push still timer)))
              (assign timers still)
              (put (proc-get pid) :resume (if found :ok :not-found))
              (push ready pid))

          :put-dict
            (let* ([p (proc-get pid)]
                   [key (get cmd 1)]
                   [val (get cmd 2)]
                   [old (get (get p :dict) key nil)])
              (put (get p :dict) key val)
              (put p :resume old)
              (push ready pid))

          :get-dict
            (let* ([p (proc-get pid)]
                   [key (get cmd 1)])
              (put p :resume (get (get p :dict) key nil))
              (push ready pid))

          :erase-dict
            (let* ([p (proc-get pid)]
                   [key (get cmd 1)]
                   [old (get (get p :dict) key nil)])
              (del (get p :dict) key)
              (put p :resume old)
              (push ready pid))

          (error {:error :protocol-error
                  :message (string "unknown scheduler command: " tag)})))))

    # ---- signal dispatch (shared by run-one and complete-io) ----

    (var dispatch-signal nil)
    (assign dispatch-signal (fn [pid f]
      (let ([bits (fiber/bits f)])
        (cond
          # Completed normally
          ((= (fiber/status f) :dead)
            (process-exit pid [:normal (fiber/value f)]))

          # Error
          ((not (= 0 (bit/and bits 1)))
            (process-exit pid [:error (fiber/value f)]))

          # Fuel exhaustion — re-queue for next round
          ((not (= 0 (bit/and bits 4096)))
            (push ready pid))

          # I/O — submit to async backend, park process
          ((not (= 0 (bit/and bits 512)))
            (let ([[ok? id] (protect (io/submit backend (fiber/value f)))])
              (if ok?
                (put io-pending id @{:pid pid})
                (begin (fiber/abort f id) (dispatch-signal pid f)))))

          # Wait — structured concurrency (ev/join, ev/select, ev/abort)
          ((not (= 0 (bit/and bits 16384)))
            (handle-wait pid (fiber/value f)))

          # Yield — scheduler command
          ((not (= 0 (bit/and bits 2)))
            (handle-cmd pid (fiber/value f)))

          (true
            (error {:error :scheduler-error
                    :message "unexpected signal bits"}))))))

    # ---- run one process ----

    (var run-one (fn [pid]
      (when (alive? pid)
        (let* ([p (proc-get pid)]
               [f (get p :fiber)]
               [resume-val (get p :resume)])
          (put p :resume nil)
          (fiber/set-fuel f quantum)
          (fiber/resume f resume-val)
          (dispatch-signal pid f)))))

    # ---- I/O completion handling ----

    # Process a batch of completions, resuming/aborting process and sub-fibers.
    (var complete-io (fn [completions]
      (each completion in completions
        (let* ([id    (get completion :id)]
               [entry (get io-pending id)])
          (del io-pending id)
          (when (not (nil? entry))
            (let ([pid (get entry :pid)]
                  [sub-fiber (get entry :fiber)])
              (if (not (nil? sub-fiber))
                # Sub-fiber I/O completion
                (begin
                  (if (nil? (get completion :error))
                    (fiber/resume sub-fiber (get completion :value))
                    (fiber/abort sub-fiber (get completion :error)))
                  (handle-sub-fiber-after-resume sub-fiber pid))
                # Process fiber I/O completion
                (when (alive? pid)
                  (let* ([p (proc-get pid)]
                         [f (get p :fiber)])
                    (fiber/set-fuel f quantum)
                    (if (nil? (get completion :error))
                      (fiber/resume f (get completion :value))
                      (fiber/abort f (get completion :error)))
                    (dispatch-signal pid f))))))))))

    # Non-blocking reap: drain any already-completed I/O.
    (var reap-io (fn []
      (when (> (length io-pending) 0)
        (complete-io (io/reap backend)))))

    # ---- main loop ----

    (var has-work? (fn []
      (or (not (empty? ready))
          (not (empty? waiting))
          (> (length io-pending) 0)
          (> (length sub-runnable) 0)
          (> (length join-waiting) 0)
          (> (length select-sets) 0))))

    (var sched-run (fn [init]
      (sched-spawn init)

      (while (has-work?)
        (assign tick (box (+ (unbox tick) 1)))
        (fire-timers)
        (reap-io)
        (drain-sub-runnable)
        (wake-waiting)

        (when (empty? ready)
          (cond
            # Nothing alive anywhere — done
            ((not (has-work?))
              nil)  # while condition will terminate

            # I/O in flight — block until completions arrive
            ((> (length io-pending) 0)
              (complete-io (io/wait backend (- 0 1)))
              (drain-sub-runnable))

            # Waiting with timers — fast-forward tick
            ((not (empty? timers))
              (var min-fire (get (get timers 0) :fire-at))
              (each timer in timers
                (when (< (get timer :fire-at) min-fire)
                  (assign min-fire (get timer :fire-at))))
              (assign tick (box min-fire))
              (fire-timers)
              (wake-waiting)
              (when (and (empty? ready) (not (empty? waiting)))
                (error {:error :deadlock
                        :message "all processes waiting, no messages pending"})))

            # Waiting with no timers, no I/O — deadlock
            ((not (empty? waiting))
              (error {:error :deadlock
                      :message "all processes waiting, no messages pending"}))))

        (let ([batch ready])
          (assign ready @[])
          (each pid in batch
            (run-one pid))))))

    # ---- external API ----

    (var sched-inject (fn [pid msg]
      (when (< pid (length procs))
        (deliver pid msg))))

    (var sched-process-info (fn [pid]
      (when (< pid (length procs))
        (let ([p (proc-get pid)])
          {:pid (get p :pid)
           :status (get p :status)
           :name (get p :name)
           :mbox-size (length (get p :mbox))
           :links (get p :links)
           :trapping (get p :trapping)}))))

    # Return scheduler struct
    {:run          sched-run
     :spawn        sched-spawn
     :inject       sched-inject
     :process-info sched-process-info
     :backend      backend}))


# ============================================================================
# Convenience functions
# ============================================================================

(defn run [sched init]
  "Run init-closure as the first process on the given scheduler."
  ((get sched :run) init))

(defn start [init &named fuel backend]
  "Create a scheduler and run init-closure. Blocks until all processes complete."
  (let ([sched (make-scheduler :fuel fuel :backend backend)])
    ((get sched :run) init)
    sched))

(defn process-info [sched pid]
  "Query process state from outside."
  ((get sched :process-info) pid))

(defn inject [sched pid msg]
  "Send a message from outside the scheduler."
  ((get sched :inject) pid msg))


# ============================================================================
# Exports
# ============================================================================

(fn []
  {# Process API (used inside processes)
   :send          send
   :recv          recv
   :recv-match    recv-match
   :recv-timeout  recv-timeout
   :self          self
   :spawn         spawn
   :spawn-link    spawn-link
   :spawn-monitor spawn-monitor
   :link          link
   :unlink        unlink
   :monitor       monitor
   :demonitor     demonitor
   :trap-exit     trap-exit
   :exit          exit
   :register      register
   :unregister    unregister
   :whereis       whereis
   :send-named    send-named
   :send-after    send-after
   :cancel-timer  cancel-timer
   :put-dict      put-dict
   :get-dict      get-dict
   :erase-dict    erase-dict

   # External API
   :make-scheduler make-scheduler
   :run            run
   :start          start
   :process-info   process-info
   :inject         inject})
