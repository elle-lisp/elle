(elle/epoch 13)
# audited: 2026-09-29
# A cancelled I/O operation leaves nothing a later operation in the program can trip on.
# docs/io.md
#
# `ev/timeout` cancels an operation on every call: whichever of the body
# and the timer loses is aborted, and aborting a fiber parked in I/O
# cancels its submission. So cancellation is not a rare path — it runs
# twice per timeout — and a cancelled operation must leave nothing a later
# operation can trip on.
#
# What this file asserts is what a program can see:
#
#   1. Each timeout answers with the winner: the body's value, nil, or a
#      `:timeout` error.
#   2. A socket whose reads were aborted still delivers the next bytes the
#      peer sends to the next read.
#   3. A descriptor number is not handed to a new socket while an
#      abandoned operation still names it. If the number goes back to the
#      OS while an operation still names it, a new socket can be handed
#      that number and the stale operation reads it — and those bytes
#      reach no fiber, because the fiber that asked for them is gone.
#
# The worker threads a cancelled operation gives back are this
# implementation's resource; tests/impl/io-cancel-workers.lisp counts them.
#
# See src/io/AGENTS.md.

# Enough iterations that a stale operation per iteration would show.
(def rounds 80)

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

# ── 1. A timer the body outran ────────────────────────────────────────

(println "timers whose body won...")

(each i in (range 0 rounds)
  (let [r (ev/timeout 30 (fn [] (+ i 1)))]
    (assert (= r (+ i 1)) (string "timeout " i ": body's value came back"))))

# ── 2. A body the timer outran ────────────────────────────────────────

(println "bodies whose timer won...")

(each i in (range 0 rounds)
  (assert (nil? (ev/timeout 0.001 (fn [] (ev/sleep 30))))
          (string "timeout " i ": the deadline won")))

# ── 3. An aborted read leaves the socket usable ───────────────────────
#
# The read can never complete on its own — nothing is ever written to
# the peer — so only the abort can end it.

(println "aborted reads...")

(let [[client server listener] (tcp-pair)]
  (each i in (range 0 rounds)
    (let [f (ev/spawn (fn [] (port/read client 8)))]
      (ev/sleep 0.001)
      (ev/abort f)))
  # The aborted reads left nothing behind that would consume what the
  # peer sends next.
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

# ── 5. A wait on a child that outlives its deadline ───────────────────

(println "waits on a child that outlives them...")

(each i in (range 0 10)
  (let [child (subprocess/exec "sleep" ["30"])]
    (assert (nil? (ev/timeout 0.05 (fn [] (subprocess/wait child))))
            (string "wait " i ": the deadline won"))
    (subprocess/kill child :sigkill)
    (subprocess/wait child)))

# ── 6. An open of a fifo nobody reads ends at its deadline ────────────
#
# `open(2)` on a fifo for writing waits until a reader opens the other
# end, which it need never do. Nothing here ever opens the read end, so
# the deadline is the only thing that ends each open.

(println "opens of a fifo nobody reads...")

(defn opens-that-only-a-deadline-ends [dir]
  "Ten opens of a fresh fifo in DIR, each ended by its own deadline."
  (let [path (concat dir "/fifo")
        made (subprocess/wait (subprocess/exec "mkfifo" [path]))]
    (assert (= 0 made) "mkfifo made the fifo")
    (each i in (range 0 10)
      (let [outcome (protect (port/open path :write :timeout 50))]
        (assert (not (get outcome 0))
                (string "open " i ": a fifo nobody reads must signal"))
        (assert (= (get (get outcome 1) :error) :timeout)
                (string "open " i ": expected a :timeout error, got "
                        (string (get outcome 1))))))))

(with-temp-dir dir (opens-that-only-a-deadline-ends dir))

(println "io cancel: every cancelled operation left the program's I/O usable")
