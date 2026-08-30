(elle/epoch 12)
# An operation whose asking fiber is gone ends without its peer acting.
#
# `a_completion_is_withheld_when_the_fiber_that_asked_is_gone`
# (src/io/aio/tests/park.rs) holds the other half of the rule: the completion
# for such an operation is retired unread and answered with an error. That half
# is only assertable there, because the answer goes to a fiber that is gone.
# This file is about the completion ARRIVING at all, which a program can see.
# The victim below parks in a read and is cancelled, and nobody ever writes to
# the connection it was reading, so the runtime is the only thing left that can
# end the operation.
#
# The trap: with a peer write this file would pass whether or not the runtime
# ends anything, because the peer's bytes wake the parked worker on their own.
# Writing nothing is what makes the assertion mean what it says.
#
# The counter-factual: with the sweep removed, the loop keeps the read
# (`:io` stays 1) and, on the thread-pool backend, keeps the worker thread
# (`:workers` stays 1) for as long as the program is pumped.
#
# The other trap: `fiber/cancel` is the route, and `fiber/abort` is not
# interchangeable with it. An abort resumes the fiber to unwind, and that
# unwinding can suspend and be resumed again (docs/signals/primitives.md
# § "Unwinding that suspends"), so the fiber is `:paused` rather than terminal
# and still has a result to come back for — the sweep leaves it alone and the
# read stays submitted, which is the same red this file reports for a missing
# sweep, from the opposite cause. Cancel gives the fiber no such chance.
#
# The victim is also `:paused` in its CONNECT before it is in its read, and
# cancelling the connect would measure that instead. So the victim says when it
# is past the connect, and the wait below reads that flag beside the status.

# Bound on how long the victim may take to reach its read, and on the loop
# letting the orphaned read go. Both are waited for by polling, so these only
# cap a failure.
(def tries 300)
(def tick 0.01)

(defn wait-until [pred]
  "Pump the loop until PRED holds, or the bound runs out. Returns whether
   it held — the caller asserts, so a timeout reads as the condition it
   was waiting for rather than as a hang."
  (let [@n 0]
    (while (and (< n tries) (not (pred)))
      (ev/sleep tick)
      (assign n (+ n 1)))
    (pred)))

(def listener (tcp/listen "127.0.0.1" 0))
(def parts (string/split (port/path listener) ":"))
(def lport (parse-int (get parts (- (length parts) 1))))

# The peer half, kept out here so it outlives the victim. It never writes.
(def @accepted nil)
(def acceptor (ev/spawn (fn [] (assign accepted (tcp/accept listener)))))

(println "an operation whose fiber is gone ends with no peer acting...")

# The victim owns its connection: the port is born in the victim's own
# region, as is the buffer the read reserves.
(def @reading false)
(def victim
  (ev/spawn (fn []
              (let [c (tcp/connect "127.0.0.1" lport)]
                (assign reading true)
                (port/read-line c)))))

(ev/join acceptor)
(assert (wait-until (fn [] (and reading (= (fiber/status victim) :paused))))
        "the victim is parked in its read")

# Nothing tells the scheduler the victim's operation is gone: the cancel ends
# the fiber with the read still submitted, and the regions holding the port and
# the read buffer are released as it unwinds.
(protect (fiber/cancel victim))
(assert (= (fiber/status victim) :error)
        "the victim ended without running to its own result")

# The read is the only operation left, and no byte will ever arrive on it.
# The loop reporting none outstanding is the runtime having ended it; the
# worker count reporting none is that thread coming back.
(defn settled []
  (let [r (ev/report)]
    (and (= 0 (get r :io)) (= 0 (get r :workers)))))

(assert (wait-until settled)
        "the orphaned read ends and gives its worker back, with no peer acting")

# The error the stale operation answered with is built from nothing the entry
# held, and it is the scheduler's to drop — the fiber it would have gone to is
# what went away. So the loop keeps running and the join is reachable.
(assert (not (nil? (protect (ev/join victim))))
        "joining the cancelled victim answers rather than hanging the loop")

(port/close accepted)
(port/close listener)

(println "tests/elle/io-stale-operation-ends.lisp: passed")
