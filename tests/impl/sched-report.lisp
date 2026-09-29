(elle/epoch 12)
# What the scheduler is waiting for, as a value.
#
# The event loop blocks when nothing is runnable and something is still
# outstanding. From inside the program that is indistinguishable from a
# wedge: both look like a run that stops producing output. `ev/report`
# separates them by naming the waits — the I/O it submitted, the joins
# and selects it holds, and every park queue with a fiber in it.
#
# A timer is what makes a report reachable. A sleep completion arrives
# without any other fiber running, so a watchdog spawned as
# `(ev/spawn (fn [] (ev/sleep n) (ev/report)))` still runs when every
# other fiber is parked. Each case below arranges one wait, reports from
# such a watchdog, and asserts the report names it.
#
# See docs/scheduler.md § "ev/report".

(def deadline 5)

(defn report-after [seconds]
  "Report from a watchdog fiber that wakes after `seconds`."
  (ev/join (ev/spawn (fn []
                       (ev/sleep seconds)
                       (ev/report)))))

# ── 1. The shape ─────────────────────────────────────────────────────

(println "report shape...")

(let [r (ev/report)]
  (assert (struct? r) "report is a struct")
  (each k in [:runnable :io :joins :selects :forwarded]
    (assert (int? (get r k)) (string "report: " (string k) " is an int")))
  (assert (>= (get r :io) 0) "report: io is not negative")
  (assert (not (nil? (get r :parks))) "report: parks is present"))

# ── 2. A parked fiber shows up under its key ─────────────────────────

(println "a park queue names its key and its depth...")

(let* [key (gensym)
       bx (box 0)
       park (fn [] (ev/spawn (fn [] (ev/futex-wait key bx 0))))
       a (park)
       b (park)
       r (report-after 0.05)
       matches (filter (fn [p] (= (get p 0) key)) (get r :parks))
       entry (if (empty? matches) nil (first matches))]
  (assert (not (nil? entry)) "report: the park key is listed")
  (assert (= (get entry 1) 2)
          (string "report: two waiters, got " (string (get entry 1))))
  (rebox bx 1)
  (ev/futex-wake key 2)
  (assert (not (nil? (ev/timeout deadline (fn [] (ev/join [a b])))))
          "the woken fibers finished"))

# ── 3. A key with no waiter left is not listed ───────────────────────

(println "a drained park queue drops its key...")

(let* [key (gensym)
       bx (box 0)
       f (ev/spawn (fn [] (ev/futex-wait key bx 0)))
       _ (ev/sleep 0.05)
       _ (rebox bx 1)
       _ (ev/futex-wake key 1)
       _ (ev/join f)
       r (ev/report)]
  (assert (empty? (filter (fn [p] (= (get p 0) key)) (get r :parks)))
          "report: the drained key is gone"))

# ── 4. Outstanding I/O is counted ────────────────────────────────────

(println "an in-flight operation is counted under :io...")

(let* [f (ev/spawn (fn [] (ev/sleep 30)))
       r (report-after 0.05)]
  # The sleeper's timer and the watchdog's own timer are both submitted;
  # the watchdog's has already completed, so the sleeper's remains.
  (assert (>= (get r :io) 1)
          (string "report: io counts the sleep, got " (string (get r :io))))
  (ev/abort f))

# ── 5. A join waiter is counted ──────────────────────────────────────

(println "a fiber with a join waiter is counted under :joins...")

(let* [key (gensym)
       bx (box 0)
       target (ev/spawn (fn [] (ev/futex-wait key bx 0)))
       joiner (ev/spawn (fn [] (ev/join-protected target)))
       r (report-after 0.05)]
  (assert (>= (get r :joins) 1) "report: the join waiter is counted")
  (rebox bx 1)
  (ev/futex-wake key 1)
  (assert (not (nil? (ev/timeout deadline (fn [] (ev/join [target joiner])))))
          "the joined fibers finished"))

(println "sched report: all cases named their wait")
