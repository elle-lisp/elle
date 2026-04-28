(elle/epoch 9)
## lib/process.lisp — Erlang-inspired process module
##
## Loaded via: (def process ((import "std/process")))
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

(defn send [pid msg]
  (yield [:send pid msg]))
(defn recv []
  (yield [:recv]))
(defn recv-match [pred]
  (yield [:recv-match pred]))
(defn recv-timeout [ticks]
  (yield [:recv-timeout ticks]))
(defn self []
  (yield [:self]))
(defn spawn [closure]
  (yield [:spawn closure]))
(defn spawn-link [closure]
  (yield [:spawn-link closure]))
(defn spawn-monitor [closure]
  (yield [:spawn-monitor closure]))
(defn link [pid]
  (yield [:link pid]))
(defn unlink [pid]
  (yield [:unlink pid]))
(defn monitor [pid]
  (yield [:monitor pid]))
(defn demonitor [ref]
  (yield [:demonitor ref]))
(defn trap-exit [flag]
  (yield [:trap-exit flag]))
(defn exit [pid reason]
  (yield [:exit pid reason]))
(defn register [name]
  (yield [:register name]))
(defn unregister [name]
  (yield [:unregister name]))
(defn whereis [name]
  (yield [:whereis name]))
(defn send-named [name msg]
  (yield [:send-named name msg]))
(defn send-after [ticks pid msg]
  (yield [:send-after ticks pid msg]))
(defn cancel-timer [ref]
  (yield [:cancel-timer ref]))
(defn put-dict [key val]
  (yield [:put-dict key val]))
(defn get-dict [key]
  (yield [:get-dict key]))
(defn erase-dict [key]
  (yield [:erase-dict key]))


# ============================================================================
# Fresh-ref generator
# ============================================================================

(defn make-ref-gen []
  (def @counter 0)
  (fn []
    (let [r counter]
      (assign counter (+ counter 1))
      r)))


# ============================================================================
# Scheduler
# ============================================================================

(defn make-scheduler [&named fuel backend]
  (let [quantum (or fuel 1000)
        procs @[]  # @array of @struct (per-process state)
        @ready @[]  # PIDs ready to run
        @waiting @[]  # PIDs blocked on recv
        io-pending @{}  # submission-id → {:pid pid} or {:fiber fiber :pid pid}
        names @{}  # name → pid
        @timers @[]  # @array of @struct {:ref :fire-at :pid :msg}
        @tick (box 0)  # logical tick (boxed for mutation in closures)
        gen-ref (make-ref-gen)
        backend (or backend (io/backend :async))  # ---- structured concurrency state ----
        sub-runnable @[]  # @array of @struct {:fiber :pid} — child fibers to resume
        sub-completed @{}  # fiber → :ok | :error
        join-waiting @{}  # target-fiber → @[pid ...] — who's joining
        select-sets @{}]
    (def @sched-spawn
      (fn [closure]
        (let [pid (length procs)
              fiber (fiber/new closure |:yield :error :fuel :io :exec :wait|)]
          (push procs
            @{:pid pid
              :fiber fiber
              :mbox @[]
              :resume nil
              :status :alive
              :links @||
              :monitors @{}
              :monitored-by @{}
              :trapping false
              :name nil
              :dict @{}
              :save-queue @[]
              :recv-pred nil})
          (push ready pid)
          pid)))

    # ---- helpers ----

    (def @proc-get (fn [pid] (get procs pid)))
    (def @alive? (fn [pid] (= (get (proc-get pid) :status) :alive)))
    (def @deliver
      (fn [pid msg]
        (when (alive? pid)
          (push (get (proc-get pid) :mbox) msg))))

    # Generate a unique ref
    (def @fresh-ref (fn [] (gen-ref)))

    # ---- links & monitors ----

    (def @add-link
      (fn [a b]
        (let [la (get (proc-get a) :links)
              lb (get (proc-get b) :links)]
          (put la b)
          (put lb a))))
    (def @remove-link
      (fn [a b]
        (let [la (get (proc-get a) :links)
              lb (get (proc-get b) :links)]
          (del la b)
          (del lb a))))
    (def @add-monitor
      (fn [watcher target]
        (let [ref (fresh-ref)
              wp (proc-get watcher)
              tp (proc-get target)]
          (put (get wp :monitors) ref target)
          (put (get tp :monitored-by) ref watcher)
          ref)))
    (def @remove-monitor
      (fn [ref watcher-pid]
        (let [wp (proc-get watcher-pid)]
          (when (has? (get wp :monitors) ref)
            (let [target (get (get wp :monitors) ref)]
              (del (get wp :monitors) ref)
              (when (alive? target)
                (del (get (proc-get target) :monitored-by) ref)))))))

    # ---- exit propagation ----

    (def @notify-links nil)
    (assign
      notify-links
      (fn [dead-pid reason]
        (each linked-pid in (get (proc-get dead-pid) :links)
          (when (alive? linked-pid)
            (if (get (proc-get linked-pid) :trapping)
              (deliver linked-pid [:EXIT dead-pid reason])
              (begin
                (put (proc-get linked-pid) :status :dead)
                (notify-links linked-pid [:linked dead-pid reason])))))))
    (def @notify-monitors
      (fn [dead-pid reason]
        (each ref in (keys (get (proc-get dead-pid) :monitored-by))
          (let [watcher (get (get (proc-get dead-pid) :monitored-by) ref)]
            (when (alive? watcher)
              (deliver watcher [:DOWN ref dead-pid reason])
              (del (get (proc-get watcher) :monitors) ref))))))
    (def @unregister-name
      (fn [pid]
        (let* [p (proc-get pid)
               n (get p :name)]
          (when n
            (del names n)
            (put p :name nil)))))

    # Cancel any in-flight I/O for a process (includes sub-fiber I/O)
    (def @cancel-process-io
      (fn [pid]
        (def @to-cancel @[])
        (each [id entry] in (pairs io-pending)
          (when (= (get entry :pid) pid) (push to-cancel id)))
        (each id in to-cancel
          (del io-pending id)
          (io/cancel backend id))))
    (def @process-exit
      (fn [pid reason]
        (put (proc-get pid) :status :dead)
        (cancel-process-io pid)
        (unregister-name pid)
        (notify-links pid reason)
        (notify-monitors pid reason)))

    # ---- timers ----

    (def @fire-timers
      (fn []
        (let [still @[]
              now (unbox tick)]
          (each timer in timers
            (if (>= now (get timer :fire-at))
              (deliver (get timer :pid) (get timer :msg))
              (push still timer)))
          (assign timers still))))

    # ---- selective receive helpers ----

    # Scan mailbox for a message matching pred, moving non-matches to save-queue
    (def @scan-mbox
      (fn [pid pred]
        (let* [p (proc-get pid)
               mbox (get p :mbox)
               save (get p :save-queue)]
          (def @found nil)
          (while (and (nil? found) (> (length mbox) 0))
            (let [msg (get mbox 0)]
              (remove mbox 0)
              (if (pred msg) (assign found msg) (push save msg))))
          found)))

    # Restore save-queue to front of mailbox
    (def @restore-save-queue
      (fn [pid]
        (let* [p (proc-get pid)
               save (get p :save-queue)
               mbox (get p :mbox)]
          (def @i (- (length save) 1))
          (while (>= i 0)  # Insert at front of mbox
            (let [tmp @[]]
              (push tmp (get save i))
              (each m in mbox
                (push tmp m))  # Replace mbox contents
              (while (> (length mbox) 0) (pop mbox))
              (each m in tmp
                (push mbox m)))
            (assign i (- i 1)))  # Clear save queue
          (while (> (length save) 0) (pop save))
          (put p :recv-pred nil))))

    # ---- waking ----

    (def @wake-waiting
      (fn []
        (let [still @[]]
          (each pid in waiting
            (if (not (alive? pid))
              nil  # skip dead
              (let* [p (proc-get pid)
                     mbox (get p :mbox)
                     pred (get p :recv-pred)]
                (if (nil? pred)  # Plain recv — take first message
                  (if (> (length mbox) 0)
                    (begin
                      (let [msg (get mbox 0)]
                        (remove mbox 0)
                        (put p :resume msg)
                        (push ready pid)))
                    (push still pid))  # Selective recv — scan for matching message
                  (let [found (scan-mbox pid pred)]
                    (if (not (nil? found))
                      (begin
                        (restore-save-queue pid)
                        (put p :resume found)
                        (push ready pid))
                      (push still pid)))))))
          (assign waiting still))))

    # ---- structured concurrency: sub-fiber completion ----

    (def @complete-sub-fiber
      (fn [fiber status]
        "Record sub-fiber completion, wake join and select waiters."
        (put sub-completed fiber status)

        # Wake join waiters
        (let [waiters (get join-waiting fiber)]
          (when (not (nil? waiters))
            (del join-waiting fiber)
            (let [pair [(= status :ok) (fiber/value fiber)]]
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
    (def @handle-sub-fiber-after-resume nil)
    (assign
      handle-sub-fiber-after-resume
      (fn [fiber pid]
        "Route a sub-fiber after resume — same logic as ev/run's handle-fiber-after-resume."
        (case (fiber/status fiber)
          :dead (complete-sub-fiber fiber :ok)
          :error (complete-sub-fiber fiber :error)
          :paused
            (let [bits (fiber/bits fiber)]
              (cond
                (not (= 0 (bit/and bits 1)))  # SIG_ERROR
                 (complete-sub-fiber fiber :error)
                (not (= 0 (bit/and bits 512)))  # SIG_IO
                (let [[ok? result] (protect (io/submit backend
                        (fiber/value fiber)))]
                  (if ok?
                    (put io-pending result @{:fiber fiber :pid pid})
                    (begin
                      (fiber/abort fiber result)
                      (handle-sub-fiber-after-resume fiber pid))))
                (push sub-runnable @{:fiber fiber :pid pid}))))))

    # Drain sub-fiber runnable queue
    (def @drain-sub-runnable
      (fn []
        (while (> (length sub-runnable) 0)
          (let* [entry (pop sub-runnable)
                 fiber (get entry :fiber)
                 pid (get entry :pid)
                 status (fiber/status fiber)]
            (cond
              (= status :dead) (complete-sub-fiber fiber :ok)
              (= status :error) (complete-sub-fiber fiber :error)
              (begin
                (fiber/resume fiber)
                (handle-sub-fiber-after-resume fiber pid)))))))

    # ---- structured concurrency: wait dispatch ----

    (def @handle-wait
      (fn [pid request]
        (case (get request :op)
          :join
            (let [target (get request :fiber)]
              (let [comp (get sub-completed target)]
                (if (not (nil? comp))  # Already completed
                  (begin
                    (put (proc-get pid)
                      :resume [(= comp :ok) (fiber/value target)])
                    (push ready pid))  # Check raw fiber status
                  (let [status (fiber/status target)]
                    (cond
                      (= status :dead)
                        (begin
                          (put (proc-get pid)
                            :resume [true (fiber/value target)])
                          (push ready pid))
                      (= status :error)
                        (begin
                          (put (proc-get pid)
                            :resume [false (fiber/value target)])
                          (push ready pid))
                      (let [ws (or (get join-waiting target)
                              (let [w @[]]
                                (put join-waiting target w)
                                w))]
                        (push ws pid)  # Ensure the target fiber gets pumped
                        (when (nil? (get sub-completed target))
                          (push sub-runnable @{:fiber target :pid pid}))))))))
          :select
            (let [candidates (get request :fibers)]
              (let [done (find (fn [f]
                                 (or (not (nil? (get sub-completed f)))
                                   (= (fiber/status f) :dead)
                                   (= (fiber/status f) :error))) candidates)]
                (if (not (nil? done))
                  (begin
                    (put (proc-get pid) :resume done)
                    (push ready pid))
                  (begin
                    (put select-sets pid @{:candidates candidates :woken false})  # Ensure all candidates get pumped
                    (each f in candidates
                      (when (nil? (get sub-completed f))
                        (push sub-runnable @{:fiber f :pid pid})))))))
          :abort
            (let [target (get request :fiber)]
              (let [comp (get sub-completed target)]
                (if (not (nil? comp))  # Already completed — no-op
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

    (def @handle-cmd
      (fn [pid cmd]
        (let [tag (get cmd 0)]
          (case tag
            :send
              (let [target (get cmd 1)
                    msg (get cmd 2)]
                (deliver target msg)
                (put (proc-get pid) :resume :ok)
                (push ready pid))
            :recv
              (let* [p (proc-get pid)
                     mbox (get p :mbox)]
                (if (> (length mbox) 0)
                  (begin
                    (let [msg (get mbox 0)]
                      (remove mbox 0)
                      (put p :resume msg)
                      (push ready pid)))
                  (push waiting pid)))
            :recv-match
              (let* [pred (get cmd 1)
                     p (proc-get pid)
                     found (scan-mbox pid pred)]
                (if (not (nil? found))
                  (begin
                    (restore-save-queue pid)
                    (put p :resume found)
                    (push ready pid))
                  (begin
                    (put p :recv-pred pred)
                    (push waiting pid))))
            :recv-timeout
              (let* [ticks (get cmd 1)
                     p (proc-get pid)
                     mbox (get p :mbox)]
                (if (> (length mbox) 0)
                  (begin
                    (let [msg (get mbox 0)]
                      (remove mbox 0)
                      (put p :resume msg)
                      (push ready pid)))
                  (let [ref (fresh-ref)
                        fire-at (+ (unbox tick) ticks)]
                    (push timers
                      @{:ref ref :fire-at fire-at :pid pid :msg :timeout})
                    (push waiting pid))))
            :self
              (begin
                (put (proc-get pid) :resume pid)
                (push ready pid))
            :spawn
              (let [new-pid (sched-spawn (get cmd 1))]
                (put (proc-get pid) :resume new-pid)
                (push ready pid))
            :spawn-link
              (let [new-pid (sched-spawn (get cmd 1))]
                (add-link pid new-pid)
                (put (proc-get pid) :resume new-pid)
                (push ready pid))
            :spawn-monitor
              (let* [new-pid (sched-spawn (get cmd 1))
                     ref (add-monitor pid new-pid)]
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
              (let [ref (add-monitor pid (get cmd 1))]
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
              (let [target (get cmd 1)
                    reason (get cmd 2)]
                (if (= target pid)  # Self-exit
                  (process-exit pid reason)  # Kill another process
                  (when (alive? target)
                    (if (and (get (proc-get target) :trapping)
                        (not (= reason :kill)))
                      (deliver target [:EXIT pid reason])
                      (process-exit target [:killed reason]))))
                (when (alive? pid)
                  (put (proc-get pid) :resume :ok)
                  (push ready pid)))
            :register
              (let [name (get cmd 1)]
                (put names name pid)
                (put (proc-get pid) :name name)
                (put (proc-get pid) :resume :ok)
                (push ready pid))
            :unregister
              (let [name (get cmd 1)]
                (del names name)
                (put (proc-get pid) :name nil)
                (put (proc-get pid) :resume :ok)
                (push ready pid))
            :whereis
              (let [name (get cmd 1)]
                (put (proc-get pid) :resume (get names name nil))
                (push ready pid))
            :send-named
              (let* [name (get cmd 1)
                     msg (get cmd 2)
                     target (get names name nil)]
                (when target (deliver target msg))
                (put (proc-get pid) :resume :ok)
                (push ready pid))
            :send-after
              (let* [ticks (get cmd 1)
                     target (get cmd 2)
                     msg (get cmd 3)
                     ref (fresh-ref)
                     fire-at (+ (unbox tick) ticks)]
                (push timers @{:ref ref :fire-at fire-at :pid target :msg msg})
                (put (proc-get pid) :resume ref)
                (push ready pid))
            :cancel-timer
              (let [ref (get cmd 1)
                    @found false
                    still @[]]
                (each timer in timers
                  (if (= (get timer :ref) ref)
                    (assign found true)
                    (push still timer)))
                (assign timers still)
                (put (proc-get pid) :resume (if found :ok :not-found))
                (push ready pid))
            :put-dict
              (let* [p (proc-get pid)
                     key (get cmd 1)
                     val (get cmd 2)
                     old (get (get p :dict) key nil)]
                (put (get p :dict) key val)
                (put p :resume old)
                (push ready pid))
            :get-dict
              (let* [p (proc-get pid)
                     key (get cmd 1)]
                (put p :resume (get (get p :dict) key nil))
                (push ready pid))
            :erase-dict
              (let* [p (proc-get pid)
                     key (get cmd 1)
                     old (get (get p :dict) key nil)]
                (del (get p :dict) key)
                (put p :resume old)
                (push ready pid))
            (error {:error :protocol-error
                    :message (string "unknown scheduler command: " tag)})))))

    # ---- signal dispatch (shared by run-one and complete-io) ----

    (def @dispatch-signal nil)
    (assign
      dispatch-signal
      (fn [pid f]
        (let [bits (fiber/bits f)]
          (cond  # Completed normally
            (= (fiber/status f) :dead)
              (process-exit pid [:normal (fiber/value f)])

              # Error
              (not (= 0 (bit/and bits 1)))
              (process-exit pid [:error (fiber/value f)])

              # Fuel exhaustion — re-queue for next round
              (not (= 0 (bit/and bits 4096))) (push ready pid)

            # I/O — submit to async backend, park process
            (not (= 0 (bit/and bits 512)))
              (let [[ok? id] (protect (io/submit backend (fiber/value f)))]
                (if ok?
                  (put io-pending id @{:pid pid})
                  (begin
                    (fiber/abort f id)
                    (dispatch-signal pid f))))

              # Wait — structured concurrency (ev/join, ev/select, ev/abort)
              (not (= 0 (bit/and bits 16384))) (handle-wait pid (fiber/value f))

            # Yield — scheduler command
            (not (= 0 (bit/and bits 2))) (handle-cmd pid (fiber/value f))
            (error {:error :scheduler-error :message "unexpected signal bits"})))))

    # ---- run one process ----

    (def @run-one
      (fn [pid]
        (when (alive? pid)
          (let* [p (proc-get pid)
                 f (get p :fiber)
                 resume-val (get p :resume)]
            (put p :resume nil)
            (fiber/set-fuel f quantum)
            (fiber/resume f resume-val)
            (dispatch-signal pid f)))))

    # ---- I/O completion handling ----

    # Process a batch of completions, resuming/aborting process and sub-fibers.
    (def @complete-io
      (fn [completions]
        (each completion in completions
          (let* [id (get completion :id)
                 entry (get io-pending id)]
            (del io-pending id)
            (when (not (nil? entry))
              (let [pid (get entry :pid)
                    sub-fiber (get entry :fiber)]
                (if (not (nil? sub-fiber))  # Sub-fiber I/O completion
                  (begin
                    (if (nil? (get completion :error))
                      (fiber/resume sub-fiber (get completion :value))
                      (fiber/abort sub-fiber (get completion :error)))
                    (handle-sub-fiber-after-resume sub-fiber pid))  # Process fiber I/O completion
                  (when (alive? pid)
                    (let* [p (proc-get pid)
                           f (get p :fiber)]
                      (fiber/set-fuel f quantum)
                      (if (nil? (get completion :error))
                        (fiber/resume f (get completion :value))
                        (fiber/abort f (get completion :error)))
                      (dispatch-signal pid f))))))))))

    # Non-blocking reap: drain any already-completed I/O.
    (def @reap-io
      (fn [] (when (> (length io-pending) 0) (complete-io (io/reap backend)))))

    # ---- main loop ----

    (def @has-work?
      (fn []
        (or (not (empty? ready)) (not (empty? waiting))
          (> (length io-pending) 0) (> (length sub-runnable) 0)
          (> (length join-waiting) 0) (> (length select-sets) 0))))
    (def @sched-run
      (fn [init]
        (sched-spawn init)
        (while (has-work?)
          (assign tick (box (+ (unbox tick) 1)))
          (fire-timers)
          (reap-io)
          (drain-sub-runnable)
          (wake-waiting)
          (when (empty? ready)
            (cond  # Nothing alive anywhere — done
              (not (has-work?)) nil  # while condition will terminate

              # I/O in flight — block until completions arrive
              (> (length io-pending) 0)
                (begin
                  (complete-io (io/wait backend (- 0 1)))
                  (drain-sub-runnable))

                # Waiting with timers — fast-forward tick
                (not (empty? timers))
                (begin
                  (def @min-fire (get (get timers 0) :fire-at))
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
                (not (empty? waiting)) (error {:error :deadlock
              :message "all processes waiting, no messages pending"})))
          (let [batch ready]
            (assign ready @[])
            (each pid in batch
              (run-one pid))))))

    # ---- external API ----

    (def @sched-inject
      (fn [pid msg] (when (< pid (length procs)) (deliver pid msg))))
    (def @sched-process-info
      (fn [pid]
        (when (< pid (length procs))
          (let [p (proc-get pid)]
            {:pid (get p :pid)
             :status (get p :status)
             :name (get p :name)
             :mbox-size (length (get p :mbox))
             :links (get p :links)
             :trapping (get p :trapping)}))))

    # Return scheduler struct
    {:run sched-run
     :spawn sched-spawn
     :inject sched-inject
     :process-info sched-process-info
     :backend backend}))


# ============================================================================
# Convenience functions
# ============================================================================

(defn run [sched init]
  "Run init-closure on an existing scheduler. Use this when you need to
   share a scheduler across multiple entry points or configure it separately.
   See also: start (which creates a scheduler for you)."
  ((get sched :run) init))

(defn start [init &named fuel backend]
  "Create a fresh scheduler and run init-closure as the first process.
   Blocks until all processes complete. Returns the scheduler.
   This is the primary entry point for most programs. Use `run` instead
   when you need to pre-configure or reuse a scheduler."
  (let [sched (make-scheduler :fuel fuel :backend backend)]
    ((get sched :run) init)
    sched))

(defn process-info [sched pid]
  "Query process state from outside."
  ((get sched :process-info) pid))

(defn inject [sched pid msg]
  "Send a message from outside the scheduler."
  ((get sched :inject) pid msg))


# ============================================================================
# GenServer — callback-based generic server
# ============================================================================
#
# Message protocol (internal, $-prefixed):
#   [:$call caller-pid ref request]   client → server
#   [:$cast request]                  client → server
#   [:$stop caller-pid ref reason]    client → server
#   [:$reply ref value]               server → client
#   [:$call-timeout ref]              timer  → client (self)
#
# Callbacks struct:
#   {:init        (fn [arg] state)
#    :handle-call (fn [request from state]
#                  [:reply reply state]
#                  | [:noreply state]
#                  | [:stop reason reply state])
#    :handle-cast (fn [request state]      [:noreply state] | [:stop reason state])
#    :handle-info (fn [msg state]          [:noreply state] | [:stop reason state])
#    :terminate   (fn [reason state] ...)}
#
# `from` in handle-call is [pid ref] — use gen-server-reply for deferred replies.

# ── helpers ───────────────────────────────────────────────────────────

(defn gen-make-ref []
  "Per-process monotonic ref for call correlation."
  (let [n (or (get-dict :$gen-call-ref) 0)]
    (put-dict :$gen-call-ref (+ n 1))
    n))

(defn gen-resolve [server]
  "Resolve server — pid passes through, keyword does whereis."
  (if (keyword? server)
    (let [pid (whereis server)]
      (when (nil? pid)
        (error {:error :noproc
                :message (string "no process registered as " server)}))
      pid)
    server))

# ── client API ────────────────────────────────────────────────────────

(defn gen-server-reply [from reply]
  "Send a reply to a pending call. from is the [pid ref] pair from handle-call."
  (send (get from 0) [:$reply (get from 1) reply]))

(defn gen-server-call [server request &named timeout]
  "Synchronous request-response. Blocks until the server replies."
  (let* [pid (gen-resolve server)
         ref (gen-make-ref)
         me (self)
         timer-ref (when (not (nil? timeout))
                     (send-after timeout me [:$call-timeout ref]))]
    (send pid [:$call me ref request])
    (let [reply (recv-match (fn [m]
                              (and (array? m) (>= (length m) 3)
                                (or (and (= (get m 0) :$reply) (= (get m 1) ref))
                                  (and (= (get m 0) :$call-timeout)
                                    (= (get m 1) ref))))))]
      (when (not (nil? timer-ref)) (cancel-timer timer-ref))
      (when (= (get reply 0) :$call-timeout)
        (error {:error :gen-server-timeout :message "gen-server call timed out"}))
      (get reply 2))))

(defn gen-server-cast [server request]
  "Asynchronous one-way message. Returns :ok immediately."
  (send (gen-resolve server) [:$cast request])
  :ok)

(defn gen-server-stop [server &named reason timeout]
  "Request graceful shutdown. Blocks until the server acknowledges."
  (let* [pid (gen-resolve server)
         ref (gen-make-ref)
         me (self)
         rsn (or reason :normal)
         timer-ref (when (not (nil? timeout))
                     (send-after timeout me [:$call-timeout ref]))]
    (send pid [:$stop me ref rsn])
    (let [reply (recv-match (fn [m]
                              (and (array? m) (>= (length m) 3)
                                (or (and (= (get m 0) :$reply) (= (get m 1) ref))
                                  (and (= (get m 0) :$call-timeout)
                                    (= (get m 1) ref))))))]
      (when (not (nil? timer-ref)) (cancel-timer timer-ref))
      (when (= (get reply 0) :$call-timeout)
        (error {:error :gen-server-timeout :message "gen-server stop timed out"}))
      (get reply 2))))

# ── server loop ───────────────────────────────────────────────────────

(defn gen-server-start-link [callbacks init-arg &named name]
  "Spawn a linked GenServer. Returns the pid."
  (let* [handle-call (get callbacks :handle-call)
         handle-cast (get callbacks :handle-cast)
         handle-info (or (get callbacks :handle-info)
           (fn [_msg state] [:noreply state]))
         on-terminate (or (get callbacks :terminate) (fn [_reason _state] nil))
         init-fn (get callbacks :init)]
    (spawn-link (fn []
                  (when (not (nil? name)) (register name))

                  # Initialize
                  (def @state
                    (let [result (init-fn init-arg)]
                      (if (and (array? result) (> (length result) 0))
                        (case (get result 0)
                          :ok (get result 1)
                          :stop
                            (begin
                              (on-terminate (get result 1) nil)
                              (exit (self) (get result 1))
                              nil)
                          result)
                        result)))

                  # Main loop
                  (forever
                    (let [msg (recv)]
                      (match msg
                        [:$call caller ref request]
                          (let [result (handle-call request [caller ref] state)]
                            (match result
                              [:reply reply new-state]
                                (begin
                                  (send caller [:$reply ref reply])
                                  (assign state new-state))
                              [:noreply new-state] (assign state new-state)
                              [:stop reason reply new-state]
                                (begin
                                  (send caller [:$reply ref reply])
                                  (on-terminate reason new-state)
                                  (exit (self) reason))
                              _ (error {:error :gen-server-error
                                        :message "handle-call returned invalid result"})))
                        [:$cast request]
                          (let [result (handle-cast request state)]
                            (match result
                              [:noreply new-state] (assign state new-state)
                              [:stop reason new-state]
                                (begin
                                  (on-terminate reason new-state)
                                  (exit (self) reason))
                              _ (error {:error :gen-server-error
                                        :message "handle-cast returned invalid result"})))
                        [:$stop caller ref reason]
                          (begin
                            (send caller [:$reply ref :ok])
                            (on-terminate reason state)
                            (exit (self) reason))
                        _
                          (let [result (handle-info msg state)]
                            (match result
                              [:noreply new-state] (assign state new-state)
                              [:stop reason new-state]
                                (begin
                                  (on-terminate reason new-state)
                                  (exit (self) reason))
                              _ (error {:error :gen-server-error
                                        :message "handle-info returned invalid result"}))))))))))


# ============================================================================
# Actor — simple state wrapper over GenServer
# ============================================================================

(defn actor-start-link [init-fn &named name]
  "Spawn a linked Actor. init-fn takes no args, returns initial state."
  (gen-server-start-link {:init (fn [_] (init-fn))
                          :handle-call (fn [request _from state]
                                         (case (get request 0)
                                           :get
                                             [:reply ((get request 1) state)
                                             state]
                                           :update
                                             (let [new-state ((get request 1) state)]
                                               [:reply :ok new-state])))
                          :handle-cast (fn [request state]
                                         (case (get request 0)
                                           :update
                                             [:noreply ((get request 1) state)]))}
    nil :name name))

(defn actor-get [actor fun]
  "Read a value derived from the actor's state."
  (gen-server-call actor [:get fun]))

(defn actor-update [actor fun]
  "Transform the actor's state synchronously. Returns :ok."
  (gen-server-call actor [:update fun]))

(defn actor-cast [actor fun]
  "Transform the actor's state asynchronously. Returns :ok."
  (gen-server-cast actor [:update fun]))


# ============================================================================
# Task — one-shot async work as a process
# ============================================================================
#
# Like ev/spawn but the work runs as a process with a pid, so it can be
# monitored, linked, and supervised.

(defn task-async [fun]
  "Spawn a linked process that runs fun and sends the result back. Returns [pid ref]."
  (let* [me (self)
         ref (gen-make-ref)
         [child-pid mon-ref] (spawn-monitor (fn []
           (let [result (fun)]
             (send me [:$task-result ref result]))))]
    [child-pid ref]))

(defn task-await [task &named timeout]
  "Wait for a task's result. task is [pid ref] from task-async."
  (let* [ref (get task 1)
         timer-ref (when (not (nil? timeout))
                     (send-after timeout (self) [:$call-timeout ref]))]
    (let [reply (recv-match (fn [m]
                              (and (array? m) (>= (length m) 3)
                                (or (and (= (get m 0) :$task-result)
                                    (= (get m 1) ref))
                                  (and (= (get m 0) :DOWN)
                                    (= (get m 1) (get task 1))))
                                (or (nil? timer-ref)
                                  (and (= (get m 0) :$call-timeout)
                                    (= (get m 1) ref)) true))))]
      (when (not (nil? timer-ref)) (cancel-timer timer-ref))
      (match reply
        [:$task-result _ value] value
        [:$call-timeout _ _] (error {:error :task-timeout
                                     :message "task-await timed out"})
        [:DOWN _ _ reason]
          (error {:error :task-error :message (string "task crashed: " reason)})
        _ (error {:error :task-error :message "unexpected task reply"})))))


# ============================================================================
# Supervisor — child process management
# ============================================================================
#
# Child spec: {:id :name  :start (fn [] ...)  :restart :permanent  :ready false}
#   :restart — :permanent (always), :transient (abnormal only), :temporary (never)
#   :ready   — when true, supervisor waits for supervisor-notify-ready before
#              starting the next child. Use for startup ordering.
#
# Strategies:
#   :one-for-one  — restart only the crashed child
#   :one-for-all  — restart all children when one crashes
#   :rest-for-one — restart crashed child and all children started after it
#
# Options:
#   :max-restarts N  — max restarts within the intensity period (default: unbounded)
#   :max-ticks    M  — intensity period in scheduler ticks (default: 5)
#   :logger       fn — (fn [event] ...) called on child lifecycle events
#
# Logger events:
#   {:event :child-started  :id id :pid pid}
#   {:event :child-ready    :id id :pid pid}
#   {:event :child-exited   :id id :pid pid :reason reason}
#   {:event :child-restarting :id id :attempt N}
#   {:event :max-restarts-reached :id id :shutting-down true}

(defn
  supervisor-start-link
  [children &named name strategy max-restarts max-ticks logger]
  "Spawn a linked Supervisor managing the given child specs."
  (let [parent (self)
        strat (or strategy :one-for-one)
        intensity-max max-restarts
        intensity-period (or max-ticks 5)
        log (or logger (fn [_] nil))]
    (spawn-link (fn []
                  (when (not (nil? name)) (register name))
                  (trap-exit true)

                  # Ordered child ids (preserves start order for rest-for-one)
                  (def @child-order @[])  # id → @{:pid :ref :spec}
                  (def @kids @{})  # id → @[tick tick ...] — restart history for intensity tracking
                  (def @restart-history @{})  # current tick counter (incremented each supervisor loop iteration)
                  (def @sup-tick 0)
                  (def @sup-self (self))
                  (def @start-child
                    (fn [spec]
                      (let* [start-fn (get spec :start)
                             needs-ready (get spec :ready)
                             wrapped (if needs-ready
                                       (fn []
                                         (put-dict :$supervisor-pid sup-self)
                                         (start-fn))
                                       start-fn)
                             child-pid (spawn-link wrapped)
                             ref (monitor child-pid)]
                        (put kids (get spec :id)
                          @{:pid child-pid :ref ref :spec spec})
                        (log {:event :child-started
                              :id (get spec :id)
                              :pid child-pid})  # If child declares readiness, wait for it before proceeding
                        (when needs-ready
                          (let [signal (recv-match (fn [m]
                                  (and (array? m) (>= (length m) 2)
                                    (or (and (= (get m 0) :$sup-ready)
                                        (= (get m 1) child-pid))
                                      (and (>= (length m) 3) (= (get m 0) :DOWN)
                                        (= (get m 2) child-pid))))))]
                            (when (= (get signal 0) :$sup-ready)
                              (log {:event :child-ready
                                    :id (get spec :id)
                                    :pid child-pid}))))
                        child-pid)))
                  (def @stop-child
                    (fn [id]
                      (when (has? kids id)
                        (let [info (get kids id)]
                          (exit (get info :pid) :shutdown)
                          (del kids id)))))
                  (def @should-restart?
                    (fn [policy reason]
                      (cond
                        (= policy :permanent) true
                        (= policy :transient) (match reason
                          [:normal _] false
                          _ true)
                        false)))

                  # Check restart intensity — returns true if restart is allowed
                  (def @check-intensity
                    (fn [id]
                      (if (nil? intensity-max)
                        true  # no limit configured
                        (begin
                          (let [history (or (get restart-history id) @[])]
                            (def @recent @[])
                            (each t in history
                              (when (>= t (- sup-tick intensity-period))
                                (push recent t)))
                            (push recent sup-tick)
                            (put restart-history id recent)
                            (if (> (length recent) intensity-max)
                              (begin
                                (log {:event :max-restarts-reached
                                      :id id
                                      :shutting-down true})
                                false)
                              true))))))

                  # Start all initial children in order
                  (each spec in children
                    (push child-order (get spec :id))
                    (start-child spec))

                  # Find dead child id by pid
                  (def @find-dead-id
                    (fn [dead-pid]
                      (def @found nil)
                      (each [id info] in (pairs kids)
                        (when (= (get info :pid) dead-pid) (assign found id)))
                      found))

                  # Supervision loop
                  (forever
                    (assign sup-tick (+ sup-tick 1))
                    (match (recv)
                      [:DOWN _ref dead-pid reason]
                        (let [dead-id (find-dead-id dead-pid)]
                          (when (not (nil? dead-id))
                            (log {:event :child-exited
                                  :id dead-id
                                  :pid dead-pid
                                  :reason reason})
                            (let* [info (get kids dead-id)
                                   spec (get info :spec)
                                   policy (or (get spec :restart) :permanent)]
                              (if (and (should-restart? policy reason)
                                  (check-intensity dead-id))
                                (begin
                                  (log {:event :child-restarting
                                        :id dead-id
                                        :attempt (length (or (get restart-history
                                            dead-id) @[]))})
                                  (case strat
                                    :one-for-one (start-child spec)
                                    :one-for-all
                                      (begin  # Stop all other children
                                        (each [id info] in (pairs kids)
                                          (when (not (= id dead-id))
                                            (exit (get info :pid) :shutdown)))  # Clear and restart all from specs in order
                                        (assign kids @{})
                                        (each id in child-order
                                          (let [spec (find (fn [s]
                                                (= (get s :id) id)) children)]
                                            (when (not (nil? spec))
                                              (start-child spec)))))
                                    :rest-for-one
                                      (begin  # Find position of dead child in order
                                        (def @pos 0)
                                        (def @found false)
                                        (each id in child-order
                                          (when (not found)
                                            (if (= id dead-id)
                                              (assign found true)
                                              (assign pos (+ pos 1)))))  # Stop children after the dead one
                                        (def @i (+ pos 1))
                                        (while (< i (length child-order))
                                          (let [id (get child-order i)]
                                            (stop-child id))
                                          (assign i (+ i 1)))  # Restart dead child and all after it
                                        (del kids dead-id)
                                        (assign i pos)
                                        (while (< i (length child-order))
                                          (let* [id (get child-order i)
                                            spec (find (fn [s]
                                                (= (get s :id) id)) children)]
                                            (when (not (nil? spec))
                                              (start-child spec)))
                                          (assign i (+ i 1))))))  # Not restarting — either policy says no or intensity exceeded
                                (del kids dead-id)))))
                      [:$sup-start-child caller ref spec]  # DynamicSupervisor: add child at runtime
                      (let [child-pid (start-child spec)]
                        (push child-order (get spec :id))
                        (send caller [:$reply ref child-pid]))
                      [:$sup-stop-child caller ref id]  # DynamicSupervisor: remove child at runtime
                      (begin
                        (stop-child id)
                        (send caller [:$reply ref :ok]))
                      [:$sup-which-children caller ref]
                        (begin
                          (def @result @[])
                          (each [id info] in (pairs kids)
                            (push result {:id id :pid (get info :pid)}))
                          (send caller [:$reply ref (freeze result)]))
                      [:EXIT from-pid _reason]
                        (when (= from-pid parent)
                          (each [_id info] in (pairs kids)
                            (exit (get info :pid) :shutdown))
                          (exit (self) :shutdown))
                      _ nil))))))

# ── DynamicSupervisor client API ──────────────────────────────────────

(defn supervisor-start-child [sup spec]
  "Add a child to a running supervisor. Returns the child pid."
  (let* [pid (gen-resolve sup)
         ref (gen-make-ref)
         me (self)]
    (send pid [:$sup-start-child me ref spec])
    (let [reply (recv-match (fn [m]
                              (and (array? m) (= (get m 0) :$reply)
                                (= (get m 1) ref))))]
      (get reply 2))))

(defn supervisor-stop-child [sup id]
  "Remove and stop a child by id."
  (let* [pid (gen-resolve sup)
         ref (gen-make-ref)
         me (self)]
    (send pid [:$sup-stop-child me ref id])
    (let [reply (recv-match (fn [m]
                              (and (array? m) (= (get m 0) :$reply)
                                (= (get m 1) ref))))]
      (get reply 2))))

(defn supervisor-which-children [sup]
  "List active children as [{:id :pid} ...]."
  (let* [pid (gen-resolve sup)
         ref (gen-make-ref)
         me (self)]
    (send pid [:$sup-which-children me ref])
    (let [reply (recv-match (fn [m]
                              (and (array? m) (= (get m 0) :$reply)
                                (= (get m 1) ref))))]
      (get reply 2))))

(defn supervisor-notify-ready []
  "Signal to the supervisor that this child is ready to serve.
   Call from within a child process whose spec has :ready true.
   The supervisor blocks on this signal before starting the next child."
  (let [sup-pid (get-dict :$supervisor-pid)]
    (when (not (nil? sup-pid))
      (send sup-pid [:$sup-ready (self)]))))


# ============================================================================
# EventManager — pub/sub event dispatching
# ============================================================================
#
# Handlers are structs with callbacks:
#   {:init        (fn [arg] state)
#    :handle-event (fn [event state] [:ok new-state] | [:remove state])
#    :terminate    (fn [reason state] ...)}

(defn event-manager-start-link [&named name]
  "Spawn a linked EventManager. Returns pid."
  (gen-server-start-link {:init (fn [_] @[])  # state is @array of @{:id :mod :state}
                          :handle-call (fn [request _from handlers]
                                         (match request
                                           [:add-handler mod init-arg]
                                             (let [ref (gen-make-ref)
                                               handler-state ((get mod :init) init-arg)]
                                               (push handlers
                                                 @{:id ref
                                                 :mod mod
                                                 :state handler-state})
                                               [:reply ref handlers])
                                           [:remove-handler ref]
                                             (begin
                                               (def @remaining @[])
                                               (each h in handlers
                                                 (if (= (get h :id) ref)
                                                   (let [term (get (get h :mod)
                                                       :terminate)]
                                                     (when (not (nil? term))
                                                       (term :remove (get h
                                                         :state))))
                                                   (push remaining h)))
                                               [:reply :ok remaining])
                                           [:sync-notify event]
                                             (begin
                                               (def @remaining @[])
                                               (each h in handlers
                                                 (let [result ((get (get h :mod)
                                                       :handle-event) event
                                                     (get h :state))]
                                                   (match result
                                                     [:ok new-state] (begin
                                                       (put h :state new-state)
                                                       (push remaining h))
                                                     [:remove _new-state]
                                                       (let [term (get (get h
                                                             :mod) :terminate)]
                                                         (when (not (nil? term))
                                                           (term :remove (get h
                                                             :state))))
                                                     _ (push remaining h))))
                                               [:reply :ok remaining])
                                           [:which-handlers]
                                             (begin
                                               (def @result @[])
                                               (each h in handlers
                                                 (push result
                                                   {:id (get h :id)
                                                   :mod (get h :mod)}))
                                               [:reply (freeze result) handlers])
                                           _ [:reply :unknown handlers]))
                          :handle-cast (fn [request handlers]
                                         (match request
                                           [:notify event]
                                             (begin
                                               (def @remaining @[])
                                               (each h in handlers
                                                 (let [result ((get (get h :mod)
                                                       :handle-event) event
                                                     (get h :state))]
                                                   (match result
                                                     [:ok new-state] (begin
                                                       (put h :state new-state)
                                                       (push remaining h))
                                                     [:remove _new-state]
                                                       (let [term (get (get h
                                                             :mod) :terminate)]
                                                         (when (not (nil? term))
                                                           (term :remove (get h
                                                             :state))))
                                                     _ (push remaining h))))
                                               [:noreply remaining])
                                           _ [:noreply handlers]))} nil
    :name name))

(defn event-manager-add-handler [manager mod init-arg]
  "Add a handler module to the event manager. Returns handler ref."
  (gen-server-call manager [:add-handler mod init-arg]))

(defn event-manager-remove-handler [manager ref]
  "Remove a handler by ref."
  (gen-server-call manager [:remove-handler ref]))

(defn event-manager-notify [manager event]
  "Broadcast an event to all handlers (async)."
  (gen-server-cast manager [:notify event]))

(defn event-manager-sync-notify [manager event]
  "Broadcast an event to all handlers (sync — waits for processing)."
  (gen-server-call manager [:sync-notify event]))

(defn event-manager-which-handlers [manager]
  "List registered handlers."
  (gen-server-call manager [:which-handlers]))


# ============================================================================
# Subprocess child helper
# ============================================================================

(defn make-subprocess-child [id bin args &named opts restart]
  "Create a child spec that manages an OS subprocess under a supervisor.
   The child process spawns the subprocess, blocks on subprocess/wait,
   then crashes to trigger supervisor restart on unexpected exit.

   Options:
     :opts     — options hash passed to subprocess/exec (env, cwd, etc.)
     :restart  — :permanent (default), :transient, or :temporary"
  {:id id
   :restart (or restart :permanent)
   :start (fn []
            (let [proc (subprocess/exec bin args (or opts @{}))]
              (let [code (subprocess/wait proc)]
                (when (not (= code 0))
                  (error {:error :subprocess-exit
                          :message (string id " exited with code " code)
                          :code code})))))})


# ============================================================================
# Exports
# ============================================================================

(fn []
  {
   # Process API (used inside processes)
   :send send
   :recv recv
   :recv-match recv-match
   :recv-timeout recv-timeout
   :self self
   :spawn spawn
   :spawn-link spawn-link
   :spawn-monitor spawn-monitor
   :link link
   :unlink unlink
   :monitor monitor
   :demonitor demonitor
   :trap-exit trap-exit
   :exit exit
   :register register
   :unregister unregister
   :whereis whereis
   :send-named send-named
   :send-after send-after
   :cancel-timer cancel-timer
   :put-dict put-dict
   :get-dict get-dict
   :erase-dict erase-dict

   # GenServer
   :gen-server-start-link gen-server-start-link
   :gen-server-call gen-server-call
   :gen-server-cast gen-server-cast
   :gen-server-stop gen-server-stop
   :gen-server-reply gen-server-reply

   # Actor
   :actor-start-link actor-start-link
   :actor-get actor-get
   :actor-update actor-update
   :actor-cast actor-cast

   # Task
   :task-async task-async
   :task-await task-await

   # Supervisor
   :supervisor-start-link supervisor-start-link
   :supervisor-start-child supervisor-start-child
   :supervisor-stop-child supervisor-stop-child
   :supervisor-which-children supervisor-which-children
   :supervisor-notify-ready supervisor-notify-ready
   :make-subprocess-child make-subprocess-child

   # EventManager
   :event-manager-start-link event-manager-start-link
   :event-manager-add-handler event-manager-add-handler
   :event-manager-remove-handler event-manager-remove-handler
   :event-manager-notify event-manager-notify
   :event-manager-sync-notify event-manager-sync-notify
   :event-manager-which-handlers event-manager-which-handlers

   # External API
   :make-scheduler make-scheduler
   :run run
   :start start
   :process-info process-info
   :inject inject})
