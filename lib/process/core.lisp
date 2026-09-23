(elle/epoch 12)
# audited: 2026-09-23
# The process table of one scheduler: mailboxes, links, monitors, exits, timers.
# lib/process/overview.md
# docs/processes.md

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
  (def io-pending @{})  # submission id → {:pid pid} or a sub-waiter
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
    (= (get (proc-get pid) :status) :alive))

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

  (defn remove-link [a b]
    (del (get (proc-get a) :links) b)
    (del (get (proc-get b) :links) a))

  (defn add-monitor [watcher target]
    (let [ref (fresh-ref)]
      (put (get (proc-get watcher) :monitors) ref target)
      (put (get (proc-get target) :monitored-by) ref watcher)
      ref))

  (defn remove-monitor [ref watcher]
    (let [monitors (get (proc-get watcher) :monitors)]
      (when (has? monitors ref)
        (let [target (get monitors ref)]
          (del monitors ref)
          (when (alive? target)
            (del (get (proc-get target) :monitored-by) ref))))))

  # ---- exit ----

  (defn notify-links [dead-pid reason]
    (each linked-pid in (get (proc-get dead-pid) :links)
      (when (alive? linked-pid)
        (if (get (proc-get linked-pid) :trapping)
          (deliver linked-pid [:EXIT dead-pid reason])
          (begin
            (put (proc-get linked-pid) :status :dead)
            (notify-links linked-pid [:linked dead-pid reason]))))))

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

  (defn cancel-io-where [owned?]
    "Cancel every forwarded I/O submission whose entry satisfies owned?."
    (def @to-cancel @[])
    (each [id entry] in (pairs io-pending)
      (when (owned? entry) (push to-cancel id)))
    (each id in to-cancel
      (del io-pending id)
      (emit :wait {:op :io-forward-cancel :id id})))

  (defn cancel-process-io [pid]
    "Cancel the in-flight I/O of process pid and of its sub-fibers."
    (cancel-io-where (fn [entry] (= (get entry :pid) pid))))

  (defn cancel-process-futex [pid]
    "Drop the futex parks of the sub-fibers of process pid."
    (each [key parked] in (pairs futex-parked)
      (let [keep (filter (fn [e]
                           (or (integer? e:waiter)
                               (not (= (owner e:waiter) pid)))) (->list parked))]
        (cond
          (empty? keep) (del futex-parked key)
          (< (length keep) (length parked)) (put futex-parked key (->array keep))))))

  (defn process-exit [pid reason]
    (put (proc-get pid) :status :dead)
    (when (and (array? reason) (= (first reason) :error))
      (put (proc-get pid)
           :exit-reason {:error :process-error :reason (string (get reason 1))}))
    (cancel-process-io pid)
    (cancel-process-futex pid)
    (unregister-name pid)
    (notify-links pid reason)
    (notify-monitors pid reason))

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
   :remove-link remove-link
   :add-monitor add-monitor
   :remove-monitor remove-monitor
   :process-exit process-exit
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
