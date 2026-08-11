(elle/epoch 12)
# What a cancelled I/O operation gives back.
#
# `ev/timeout` cancels an operation on every call: whichever of the body
# and the timer loses is aborted, and aborting a fiber parked in I/O
# cancels its submission. So cancellation is not a rare path — it runs
# twice per timeout — and anything a cancelled operation fails to give
# back accumulates for the life of the process.
#
# Two things must come back.
#
#   1. The worker. A thread-pool operation runs on an OS thread; one that
#      is cancelled and never reports completion keeps that thread for
#      the life of the process. Nothing caps this — the operating
#      system's own limits do — so a leak shows as growth rather than as
#      an error, and `(ev/report):workers` is what makes it visible. It
#      counts operations submitted and not yet reaped, so it returns to
#      its starting level once a loop's work is done.
#
#   2. The descriptor number. A worker resolves its fd when it runs, not
#      when it was submitted. If the number goes back to the OS while an
#      operation still names it, a new socket can be handed that number
#      and the stale operation reads it — and those bytes reach no
#      fiber, because the fiber that asked for them is gone. The peer
#      then waits forever for a reply whose request was swallowed.
#
# Both are backend-independent promises, so every case here runs the same
# on io_uring and on the thread pool. `:workers` is zero throughout on
# io_uring, which runs its operations in the kernel rather than on
# threads; the assertions below are written to hold either way.
#
# See src/io/AGENTS.md § "I/O Cancellation".

# Enough iterations that a leak of one worker per iteration is
# unmistakable against the handful outstanding at any moment.
(def rounds 80)

# How many workers a settled scheduler may still have out. The report is
# taken from a fiber the loop just resumed, so the watchdog's own timer
# and a straggler completion not yet reaped are legitimately in flight.
(def settled 4)

(defn workers []
  "Background worker operations outstanding right now."
  (get (ev/report) :workers))

(defn settled-workers []
  "Outstanding workers once the loop's own work has been reaped."
  (ev/sleep 0.05)
  (workers))

# A deadline no unblocked operation here can reach.
(def deadline 5)

(defn tcp-pair []
  "A connected [client server listener] triple over loopback."
  (let* [listener (tcp/listen "127.0.0.1" 0)
         lpath (port/path listener)
         lport (parse-int (slice lpath (+ 1 (string/find lpath ":"))))
         client (tcp/connect "127.0.0.1" lport)
         server (tcp/accept listener)]
    [client server listener]))

(defn close-all [ports]
  (each p in ports
    (protect (port/close p))))

# ── 1. A timer the body outran gives its worker back ─────────────────
#
# The body wins every time, so every iteration cancels a 30 s timer. A
# timer that ran on to its own deadline would still hold all eighty of
# them at the end of the loop.

(println "timers whose body won...")

(each i in (range 0 rounds)
  (let [r (ev/timeout 30 (fn [] (+ i 1)))]
    (assert (= r (+ i 1)) (string "timeout " i ": body's value came back"))))

(let [left (settled-workers)]
  (assert (<= left settled)
          (string "the cancelled timers gave their workers back, but "
                  (string left) " are still out")))

# ── 2. A body the timer outran gives its worker back ─────────────────
#
# The other side of the same call: here the deadline wins and the body's
# operation is the one cancelled.

(println "bodies whose timer won...")

(each i in (range 0 rounds)
  (assert (nil? (ev/timeout 0.001 (fn [] (ev/sleep 30))))
          (string "timeout " i ": the deadline won")))

(let [left (settled-workers)]
  (assert (<= left settled)
          (string "the cancelled sleeps gave their workers back, but "
                  (string left) " are still out")))

# ── 3. An aborted read gives its worker back ─────────────────────────
#
# The read can never complete on its own — nothing is ever written to
# the peer — so only the abort can end it.

(println "aborted reads...")

(let [[client server listener] (tcp-pair)]
  (each i in (range 0 rounds)
    (let [f (ev/spawn (fn [] (port/read client 8)))]
      (ev/sleep 0.001)
      (ev/abort f)))
  (let [left (settled-workers)]
    (assert (<= left settled)
            (string "the aborted reads gave their workers back, but "
                    (string left) " are still out")))
  # And the socket is still usable: the aborted reads left nothing behind
  # that would consume what the peer sends next.
  (port/write server (bytes 1 2 3 4 5 6 7 8))
  (let [got (ev/timeout deadline (fn [] (port/read client 8)))]
    (assert (not (nil? got)) "a read still completes after the aborts")
    (assert (= (length got) 8) "the read got all eight bytes"))
  (close-all [client server listener]))

# ── 4. A descriptor is not reused while an operation names it ────────
#
# Each round parks a read that nothing will satisfy, abandons it, and
# closes its socket — then opens a fresh pair, which takes the freed
# descriptor numbers back. If the abandoned read reaches the new socket,
# it eats the bytes written below and the read that follows finds
# nothing.

(println "descriptor reuse after an abandoned read...")

(each i in (range 0 40)
  (let [[client server listener] (tcp-pair)]
    (ev/abort (ev/spawn (fn [] (port/read client 64))))
    (close-all [client server listener]))
  (let [[client server listener] (tcp-pair)]
    (port/write server (bytes 11 12 13 14 15 16 17 18))
    (let [got (ev/timeout deadline (fn [] (port/read client 8)))]
      (assert (not (nil? got))
              (string "round " i ": the new socket kept its bytes"))
      (assert (= (length got) 8) (string "round " i ": all eight bytes arrived")))
    (close-all [client server listener])))

# ── 5. A cancelled wait on a child gives its worker back ─────────────
#
# `subprocess/wait` on a child that never exits waits for the life of the
# process. A supervisor gives it a deadline, and the cancel that deadline
# issues has to reach the wait — a `waitpid` the worker is already inside
# cannot be retracted.

(println "waits on a child that outlives them...")

(each i in (range 0 10)
  (let [child (subprocess/exec "sleep" ["30"])]
    (assert (nil? (ev/timeout 0.05 (fn [] (subprocess/wait child))))
            (string "wait " i ": the deadline won"))
    (subprocess/kill child :sigkill)
    (subprocess/wait child)))

(let [left (settled-workers)]
  (assert (<= left settled)
          (string "the cancelled waits gave their workers back, but "
                  (string left) " are still out")))

# ── 6. A cancelled open of a fifo gives its worker back ──────────────
#
# `open(2)` on a fifo for writing waits until a reader opens the other
# end, which it need never do. Nothing here ever opens the read end, so
# the deadline is the only thing that ends each open.

(println "opens of a fifo nobody reads...")

(let* [dir (file/mktempdir)
       path (concat dir "/fifo")]
  (assert (= 0 (subprocess/wait (subprocess/exec "mkfifo" [path])))
          "mkfifo made the fifo")
  (each i in (range 0 10)
    (let [outcome (protect (port/open path :write :timeout 50))]
      (assert (not (get outcome 0))
              (string "open " i ": a fifo nobody reads must signal"))
      (assert (= (get (get outcome 1) :error) :timeout)
              (string "open " i ": expected a :timeout error, got "
                      (string (get outcome 1))))))
  (let [left (settled-workers)]
    (assert (<= left settled)
            (string "the timed-out opens gave their workers back, but "
                    (string left) " are still out")))
  (file/delete-dir-all dir))

(println "io cancel: every cancelled operation gave back what it held")
