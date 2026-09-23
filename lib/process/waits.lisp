(elle/epoch 12)
# audited: 2026-09-23
# Structured concurrency inside processes: sub-fibers, the join, select, abort and futex waits, and relayed I/O.
# lib/process/overview.md
# docs/process-scheduler.md

(fn [core]
  (def io-pending core:io-pending)
  (def futex-parked core:futex-parked)
  (def sub-waiter core:sub-waiter)
  (def bucket core:bucket)
  (def owner core:owner)

  (def sub-runnable @[])  # sub-waiters to resume
  (def sub-completed @{})  # fiber → :ok | :error
  (def join-waiting @{})  # target fiber → @[waiter ...]
  (def select-sets @{})  # wait key → @{:candidates :woken :waiter}
  (def io-completions @[])  # completions forwarded by the parent scheduler
  (def io-wakeup-box (box 0))  # futex box the parent reboxes to wake us
  (def spawning-pid (box 0))  # the process whose code is running now

  (defn spawn-fn [fiber]
    "The *spawn* hook: queue a sub-fiber of the running process."
    (push sub-runnable (sub-waiter fiber (unbox spawning-pid)))
    fiber)

  (defn wait-key [w]
    (if (integer? w) w w:fiber))

  (defn wake [w value]
    "Resume waiter w with value. A process is queued; a sub-fiber runs now."
    (if (integer? w)
      (core:resume w value)
      (when (= (fiber/status w:fiber) :paused)
        (fiber/resume w:fiber value)
        (after-resume w:fiber w:pid))))

  (defn forward-io [request]
    "Hand an I/O request to the parent scheduler. Returns its id or [:error e]."
    (emit :wait {:op :io-forward
                 :request request
                 :queue io-completions
                 :wake-box io-wakeup-box}))

  (defn finished? [f]
    (or (not (nil? (get sub-completed f))) (= (fiber/status f) :dead)
        (= (fiber/status f) :error)))

  (defn outcome [f]
    "[ok? value] for a finished fiber."
    (let [comp (get sub-completed f)]
      (if (nil? comp)
        [(= (fiber/status f) :dead) (fiber/value f)]
        [(= comp :ok) (fiber/value f)])))

  # ---- completion ----

  (defn complete-sub-fiber [fiber status]
    "Record sub-fiber completion, wake join and select waiters."
    (put sub-completed fiber status)
    (let [waiters (get join-waiting fiber)]
      (when (not (nil? waiters))
        (del join-waiting fiber)
        (let [pair [(= status :ok) (fiber/value fiber)]]
          (each w in waiters
            (wake w pair)))))
    (each [key entry] in (pairs select-sets)
      (when (and (not (get entry :woken))
                 (not (nil? (find (fn [f] (= f fiber)) (get entry :candidates)))))
        (put entry :woken true)
        (del select-sets key)
        (wake (get entry :waiter) fiber))))

  # Queue a join/select target so it gets a first run. Only a :new fiber
  # needs this — one built by hand and never spawned, which no queue holds.
  # A :paused fiber is parked on I/O, a futex, or a wait the scheduler
  # already tracks, and resuming it out of turn hands its park a nil
  # result: a timer parked in ev/sleep would complete instantly, and every
  # ev/timeout in a process would report its deadline at once
  # (tests/elle/process-select.lisp). A fiber already queued would be
  # resumed a second time the same way.
  (defn pump-target [target pid]
    (when (and (= (fiber/status target) :new)
               (nil? (find (fn [e] (= (get e :fiber) target))
                           (->list sub-runnable))))
      (push sub-runnable (sub-waiter target pid))))

  # ---- futex ----

  (defn wake-parked [key wake?]
    "Wake the waiters parked on key that wake? picks. Returns how many woke.
     A park made while waking lands in a fresh bucket and is kept."
    (let [parked (->list (or (get futex-parked key) []))
          keep @[]
          @woken 0]
      (del futex-parked key)
      (each e in parked
        (if (wake? e woken)
          (begin
            (assign woken (+ woken 1))
            (wake e:waiter nil))
          (push keep e)))
      (each e in (or (get futex-parked key) [])
        (push keep e))
      (if (empty? keep) (del futex-parked key) (put futex-parked key keep))
      woken))

  (defn wake-futex-ready []
    "Wake every parked waiter whose futex value has changed."
    (each key in (keys futex-parked)
      (wake-parked key (fn [e _] (not (= (unbox e:val) e:expected))))))

  # ---- wait ops ----

  (defn known-op? [op]
    (has? |:join :select :abort :park :notify :io-forward :io-forward-cancel| op))

  (defn relay-io [w request]
    "Forward the I/O of a scheduler nested in waiter w to our parent, and
     remember where its completion goes. Wakes w with the id or [:error e]."
    (let [id (forward-io (get request :request))]
      (when (not (and (array? id) (= (first id) :error)))
        (put io-pending id
             @{:pid (owner w)
               :relay-fiber (when (not (integer? w)) w:fiber)
               :queue (get request :queue)
               :wake-box (get request :wake-box)}))
      (wake w id)))

  (defn cancel-relayed-io [w id]
    "Cancel I/O relayed for a nested scheduler, and wake waiter w."
    (core:cancel-io id)
    (wake w nil))

  (defn unknown-op [op]
    {:error :protocol-error :message (string "unknown :wait op: " op)})

  (defn handle-wait [w request]
    "Serve a :wait request from waiter w."
    (let [pid (owner w)]
      (case (get request :op)
        :join
          (let [target (get request :fiber)]
            (if (finished? target)
              (wake w (outcome target))
              (begin
                (push (bucket join-waiting target) w)
                (pump-target target pid))))
        :select
          (let* [candidates (get request :fibers)
                 done (find finished? candidates)]
            (if (not (nil? done))
              (wake w done)
              (begin
                (put select-sets (wait-key w)
                     @{:candidates candidates :woken false :waiter w})
                (each f in candidates
                  (pump-target f pid)))))
        :abort
          (let [target (get request :fiber)]
            (when (nil? (get sub-completed target))
              (core:cancel-io-where (fn [entry]
                                      (or (= (get entry :fiber) target)
                                      (= (get entry :relay-fiber) target))))
              (protect (fiber/abort target {:error :aborted}))
              (after-resume target pid))
            (wake w nil))
        :park
          (let [bx (get request :val)
                expected (get request :expected)]
            (if (not (= (unbox bx) expected))
              (wake w nil)
              (push (bucket futex-parked (get request :key))
                    @{:waiter w :val bx :expected expected})))
        :notify
          (let [count (get request :count)]
            (wake w (wake-parked (get request :key) (fn [_ n] (< n count)))))
        :io-forward (relay-io w request)
        :io-forward-cancel (cancel-relayed-io w (get request :id))

        ## Unknown wait op — fail loudly, in the fiber that emitted it. A
        ## silent re-queue resumes the emitting fiber with nil, which it reads
        ## as its wait's result. The scheduler checks a process fiber's op
        ## with known-op? before it hands the wait here.
        (let [err (unknown-op (get request :op))]
          (if (integer? w)
            (error err)
            (begin
              (protect (fiber/abort w:fiber err))
              (after-resume w:fiber pid)))))))

  # ---- routing a sub-fiber after it runs ----

  (defn after-resume [fiber pid]
    "Route a sub-fiber after resume — the same logic as ev/run's."
    (case (fiber/status fiber)
      :dead (complete-sub-fiber fiber :ok)
      :error (complete-sub-fiber fiber :error)
      :paused
        (let [bits (fiber/bits fiber)]
          (cond
            (not (= 0 (bit/and bits 1)))  # SIG_ERROR
             (complete-sub-fiber fiber :error)
            (not (= 0 (bit/and bits 512)))  # SIG_IO
            (let [id (forward-io (fiber/value fiber))]
              (if (and (array? id) (= (first id) :error))
                (begin
                  (fiber/abort fiber (get id 1))
                  (after-resume fiber pid))
                (put io-pending id (sub-waiter fiber pid))))
            (not (= 0 (bit/and bits 16384)))  # SIG_WAIT
             (handle-wait (sub-waiter fiber pid) (fiber/value fiber))
            (push sub-runnable (sub-waiter fiber pid))))))

  (defn drain-sub-runnable []
    (while (> (length sub-runnable) 0)
      (let* [entry (pop sub-runnable)
             fiber (get entry :fiber)
             status (fiber/status fiber)]
        (cond
          (= status :dead) (complete-sub-fiber fiber :ok)
          (= status :error) (complete-sub-fiber fiber :error)
          (begin
            (fiber/resume fiber)
            (after-resume fiber (get entry :pid)))))))

  # ---- teardown ----

  (defn can-progress? []
    "Whether a sub-fiber is queued to run, or a parked waiter's futex value
     has changed since it parked."
    (def @found (> (length sub-runnable) 0))
    (each [_key parked] in (pairs futex-parked)
      (each e in parked
        (when (not (= (unbox e:val) e:expected)) (assign found true))))
    found)

  (defn has-sub-work? []
    (or (> (length sub-runnable) 0) (> (length join-waiting) 0)
        (> (length select-sets) 0) (> (length futex-parked) 0)))

  # Every sub-fiber the scheduler still tracks, as sub-waiters. At teardown
  # (no process alive) each is an orphan to abort. Sources, in order: waiting
  # on forwarded I/O; parked on a futex (sub-fiber parks alone, because an
  # exit drops the parks of its process); queued but not yet run; awaited join
  # targets and their sub-fiber joiners; select waiters and their candidates.
  (defn collect-orphan-subs []
    (def vs @[])
    (each [_id e] in (pairs io-pending)
      (when (get e :fiber) (push vs e)))
    (each [_key parked] in (pairs futex-parked)
      (each e in (->list parked)
        (when (not (integer? e:waiter)) (push vs e:waiter))))
    (each e in (->list sub-runnable)
      (push vs e))
    (each [target waiters] in (pairs join-waiting)
      (push vs (sub-waiter target 0))
      (each w in (->list waiters)
        (when (not (integer? w)) (push vs w))))
    (each [_key e] in (pairs select-sets)
      (when (not (integer? e:waiter)) (push vs e:waiter))
      (each f in (get e :candidates)
        (push vs (sub-waiter f 0))))
    vs)

  (defn clear-map [m]
    (each k in (keys m)
      (del m k)))

  (defn clear-sub-state []
    "Cancel every forwarded submission and forget every tracked sub-fiber."
    (each id in (keys io-pending)
      (core:cancel-io id))
    (core:refill sub-runnable [])
    (clear-map futex-parked)
    (clear-map join-waiting)
    (clear-map select-sets))

  {:spawn-fn spawn-fn
   :spawning-pid spawning-pid
   :io-completions io-completions
   :io-wakeup-box io-wakeup-box
   :sub-runnable sub-runnable
   :forward-io forward-io
   :known-op? known-op?
   :unknown-op unknown-op
   :handle-wait handle-wait
   :after-resume after-resume
   :drain-sub-runnable drain-sub-runnable
   :wake-futex-ready wake-futex-ready
   :has-sub-work? has-sub-work?
   :can-progress? can-progress?
   :collect-orphan-subs collect-orphan-subs
   :clear-sub-state clear-sub-state})
