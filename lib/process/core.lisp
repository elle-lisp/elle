(elle/epoch 12)
# audited: 2026-09-23
# The process table of one scheduler: mailboxes, links, monitors, exits, timers.
# lib/process/overview.md
# docs/processes.md
# docs/process-scheduler.md

(defn make-ref-gen []
  (def @counter 0)
  (fn []
    (let [r counter]
      (assign counter (+ counter 1))
      r)))

(defn bucket [m k]
  "The array stored under k in m, created empty on first use."
  (or (get m k)
      (let [a @[]]
        (put m k a)
        a)))

(defn refill [arr items]
  "Replace the contents of arr with items, in place."
  (while (> (length arr) 0) (pop arr))
  (each x in items
    (push arr x)))

(defn sub-waiter [fiber pid]
  "A sub-fiber that process pid spawned, as the scheduler tracks it."
  @{:fiber fiber :pid pid})

(defn owner [w]
  "The pid a waiter belongs to. A process waits as its own pid."
  (if (integer? w) w w:pid))

(fn [quantum]
  (def procs @[])  # pid → @struct of per-process state
  (def ready @[])  # pids to run in the next round
  (def waiting @[])  # pids blocked in a receive
  (def names @{})  # registered name → pid
  (def timers @[])  # @{:ref :fire-at :pid :msg}
  (def clock (box 0))  # the logical tick
  (def io-pending @{})  # submission id → a process, a sub-waiter, a relay or the alarm
  (def futex-parked @{})  # futex key → @[@{:waiter :val :expected}]
  (def fresh-ref (make-ref-gen))

  (defn spawn [closure]
    (let [pid (length procs)
          fiber (fiber/new closure |:yield :error :fuel :io :exec :wait|)]
      (push procs
            @{:pid pid
              :fiber fiber
              :mbox @[]
              :resume nil
              :status :alive
              :exit-reason nil
              :links @||
              :monitors @{}
              :monitored-by @{}
              :aliases @||
              :trapping false
              :name nil
              :dict @{}
              :save-queue @[]
              :recv-pred nil
              :timer-ref nil})
      (push ready pid)
      pid))

  (defn proc-get [pid]
    (get procs pid))

  (defn alive? [pid]
    (let [p (proc-get pid)]
      (and (not (nil? p)) (= (get p :status) :alive))))

  (defn deliver [pid msg]
    (when (alive? pid)
      (push (get (proc-get pid) :mbox) msg)))

  (defn resume [pid value]
    "Queue process pid to run, handing value to its pending yield."
    (when (alive? pid)
      (put (proc-get pid) :resume value)
      (push ready pid)))

  (defn any-alive? []
    (def @found false)
    (each p in procs
      (when (= (get p :status) :alive) (assign found true)))
    found)

  # ---- links and monitors ----

  (defn add-link [a b]
    (put (get (proc-get a) :links) b)
    (put (get (proc-get b) :links) a))

  (defn link [pid target]
    "Link pid to target. Returns :noproc when target has exited, pid was not
     linked to it, and pid does not trap exits; :ok otherwise."
    (let [t (proc-get target)]
      (cond
        (alive? target) (begin
                          (add-link pid target)
                          :ok)
        (and (not (nil? t)) (has? (get t :links) pid)) :ok
        (get (proc-get pid) :trapping)
          (begin
            (deliver pid [:EXIT target :noproc])
            :ok)
        :noproc)))

  (defn remove-link [a b]
    (del (get (proc-get a) :links) b)
    (when (not (nil? (proc-get b)))
      (del (get (proc-get b) :links) a)))

  (defn add-monitor [watcher target]
    "Monitor target. A target that has exited, or never existed, sends
     [:DOWN ref target :noproc] at once."
    (let [ref (fresh-ref)]
      (if (alive? target)
        (begin
          (put (get (proc-get watcher) :monitors) ref target)
          (put (get (proc-get target) :monitored-by) ref watcher))
        (deliver watcher [:DOWN ref target :noproc]))
      ref))

  (defn remove-monitor [ref watcher]
    (let [monitors (get (proc-get watcher) :monitors)]
      (when (has? monitors ref)
        (let [target (get monitors ref)]
          (del monitors ref)
          (when (alive? target)
            (del (get (proc-get target) :monitored-by) ref))))))

  # ---- exit ----

  (defn normal? [reason]
    "Whether an exit reason is normal: :normal, or [:normal value] from a return."
    (or (= reason :normal) (and (array? reason) (= (first reason) :normal))))

  (defn notify-links [dead-pid reason]
    (each linked-pid in (get (proc-get dead-pid) :links)
      (when (alive? linked-pid)
        (cond
          (get (proc-get linked-pid) :trapping) (deliver linked-pid
          [:EXIT dead-pid reason])
          (not (normal? reason)) (process-exit linked-pid
          [:linked dead-pid reason])))))

  (defn notify-monitors [dead-pid reason]
    (let [monitored-by (get (proc-get dead-pid) :monitored-by)]
      (each ref in (keys monitored-by)
        (let [watcher (get monitored-by ref)]
          (when (alive? watcher)
            (deliver watcher [:DOWN ref dead-pid reason])
            (del (get (proc-get watcher) :monitors) ref))))))

  (defn unregister-name [pid]
    (let* [p (proc-get pid)
           n (get p :name)]
      (when n
        (del names n)
        (put p :name nil))))

  (defn cancel-io [id]
    "Cancel the forwarded I/O submission id, when it is still pending."
    (when (has? io-pending id)
      (del io-pending id)
      (emit :wait {:op :io-forward-cancel :id id})))

  (defn cancel-io-where [owned?]
    "Cancel every forwarded I/O submission whose entry satisfies owned?."
    (def @to-cancel @[])
    (each [id entry] in (pairs io-pending)
      (when (owned? entry) (push to-cancel id)))
    (each id in to-cancel
      (cancel-io id)))

  (defn cancel-process-io [pid]
    "Cancel the in-flight I/O of process pid and of its sub-fibers."
    (cancel-io-where (fn [entry] (= (get entry :pid) pid))))

  (defn cancel-process-futex [pid]
    "Drop the futex parks of process pid and of its sub-fibers."
    (each [key parked] in (pairs futex-parked)
      (let [keep (filter (fn [e] (not (= (owner e:waiter) pid))) (->list parked))]
        (cond
          (empty? keep) (del futex-parked key)
          (< (length keep) (length parked)) (put futex-parked key (->array keep))))))

  (defn imposed? [reason]
    "Whether an exit reason ends a process against its will: a raise, a link or a kill."
    (and (array? reason) (has? |:error :linked :killed| (first reason))))

  (defn process-exit [pid reason]
    (when (alive? pid)
      (put (proc-get pid) :status :dead)
      (when (imposed? reason)
        (put (proc-get pid)
             :exit-reason {:error :process-error
                           :reason (string reason)
                           :message (string "process " pid " exited: " reason)}))
      (cancel-process-io pid)
      (cancel-process-futex pid)
      (unregister-name pid)
      (notify-links pid reason)
      (notify-monitors pid reason)))

  (defn shutdown-idle []
    "Exit every process waiting in a receive, with :shutdown."
    (each pid in (->list waiting)
      (process-exit pid :shutdown))
    (refill waiting []))

  (defn flush-tagged [pid tag ref]
    "Remove every [tag ref ...] message from the mailbox of pid."
    (let [p (proc-get pid)
          keep? (fn [m]
                  (not (and (array? m) (= (get m 0) tag) (= (get m 1) ref))))]
      (refill (get p :mbox) (filter keep? (->list (get p :mbox))))
      (refill (get p :save-queue) (filter keep? (->list (get p :save-queue))))))

  # ---- aliases ----

  (defn add-alias [pid]
    "A fresh ref that reply delivers to pid until remove-alias turns it off."
    (let [ref (fresh-ref)]
      (put (get (proc-get pid) :aliases) ref)
      ref))

  (defn remove-alias [pid ref]
    "Turn off the alias ref of pid, and drop a reply to it already delivered."
    (del (get (proc-get pid) :aliases) ref)
    (flush-tagged pid :$reply ref))

  (defn reply [pid ref value]
    "Deliver [:$reply ref value] to pid while ref is one of its aliases."
    (when (and (alive? pid) (has? (get (proc-get pid) :aliases) ref))
      (deliver pid [:$reply ref value])))

  # ---- timers ----

  (defn now []
    (unbox clock))

  (defn add-timer [ticks pid msg]
    (let [ref (fresh-ref)]
      (push timers @{:ref ref :fire-at (+ (now) ticks) :pid pid :msg msg})
      ref))

  (defn cancel-timer [ref]
    "Remove the timer ref. Returns true when it was still pending."
    (let [before (length timers)]
      (refill timers
              (filter (fn [t] (not (= (get t :ref) ref))) (->list timers)))
      (< (length timers) before)))

  (defn fire-timers []
    (let [still @[]
          due @[]]
      (each timer in timers
        (if (>= (now) (get timer :fire-at)) (push due timer) (push still timer)))
      (refill timers still)
      (each timer in due
        (if (= (get timer :msg) :timeout)
          (let [p (proc-get (get timer :pid))]
            (when (= (get p :timer-ref) (get timer :ref))
              (put p :timer-ref nil)
              (deliver (get timer :pid) :timeout)))
          (deliver (get timer :pid) (get timer :msg))))))

  (defn earliest-timer []
    (def @min-fire (get (get timers 0) :fire-at))
    (each timer in timers
      (when (< (get timer :fire-at) min-fire)
        (assign min-fire (get timer :fire-at))))
    min-fire)

  # ---- receive ----

  (defn scan-mbox [pid pred]
    "Take the first message matching pred, moving earlier ones to the save queue."
    (let* [p (proc-get pid)
           mbox (get p :mbox)
           save (get p :save-queue)]
      (def @found nil)
      (while (and (nil? found) (> (length mbox) 0))
        (let [msg (get mbox 0)]
          (remove mbox 0)
          (if (pred msg) (assign found msg) (push save msg))))
      found))

  (defn restore-save-queue [pid]
    "Put the save queue back in front of the mailbox, in order."
    (let* [p (proc-get pid)
           save (get p :save-queue)
           mbox (get p :mbox)]
      (refill mbox (concat (->list save) (->list mbox)))
      (refill save [])
      (put p :recv-pred nil)))

  (defn take-message [pid]
    "The message a blocked receive of pid can take now, or nil."
    (let* [p (proc-get pid)
           mbox (get p :mbox)
           pred (get p :recv-pred)]
      (if (nil? pred)
        (when (> (length mbox) 0)
          (let [msg (get mbox 0)]
            (remove mbox 0)
            msg))
        (let [found (scan-mbox pid pred)]
          (when (not (nil? found)) (restore-save-queue pid))
          found))))

  (defn wake-waiting []
    (let [still @[]]
      (each pid in waiting
        (when (alive? pid)
          (let [msg (take-message pid)]
            (if (nil? msg)
              (push still pid)
              (begin
                (put (proc-get pid) :timer-ref nil)
                (resume pid msg))))))
      (refill waiting still)))

  {:bucket bucket
   :refill refill
   :sub-waiter sub-waiter
   :owner owner
   :procs procs
   :ready ready
   :waiting waiting
   :names names
   :timers timers
   :io-pending io-pending
   :futex-parked futex-parked
   :quantum quantum
   :fresh-ref fresh-ref
   :spawn spawn
   :proc-get proc-get
   :alive? alive?
   :deliver deliver
   :resume resume
   :any-alive? any-alive?
   :add-link add-link
   :link link
   :remove-link remove-link
   :shutdown-idle shutdown-idle
   :flush-tagged flush-tagged
   :add-alias add-alias
   :remove-alias remove-alias
   :reply reply
   :add-monitor add-monitor
   :remove-monitor remove-monitor
   :process-exit process-exit
   :cancel-io cancel-io
   :cancel-io-where cancel-io-where
   :now now
   :tick (fn [] (rebox clock (+ (now) 1)))
   :advance-to (fn [t] (rebox clock t))
   :add-timer add-timer
   :cancel-timer cancel-timer
   :fire-timers fire-timers
   :earliest-timer earliest-timer
   :scan-mbox scan-mbox
   :restore-save-queue restore-save-queue
   :wake-waiting wake-waiting})
