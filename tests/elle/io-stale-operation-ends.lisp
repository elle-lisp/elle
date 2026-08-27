(elle/epoch 12)
# An operation whose asking fiber is gone ends without its peer acting.
#
# `tests/elle/io-late-completion-port.lisp` holds the other half of the rule:
# the completion for such an operation is retired unread and answered. This
# file is about that completion arriving at all. The victim below parks in a
# read and dies, and nobody ever writes to the connection it was reading, so
# the runtime is the only thing left that can end the operation.
#
# The trap: with a peer write this file would pass whether or not the runtime
# ends anything. `poll(2)` on Linux holds a reference to the file it waits on,
# so the victim's socket outlives the close its release performed and a peer's
# bytes still reach the parked worker. Writing nothing is what makes the
# assertion mean what it says.
#
# The counter-factual: with the stale sweep removed, the loop keeps the read
# (`:io` stays 1) and, on the thread-pool backend, keeps the worker thread
# (`:workers` stays 1) for as long as the program is pumped.
#
# The other trap, shared with io-late-completion-port.lisp: the victim is
# `:paused` in its CONNECT as well as in its read, and an abort landing on the
# connect is caught by no `protect` here — it leaves the fiber in `:error` and
# the file fails with the injected payload. So the victim says when it is past
# the connect, and the wait below reads that flag beside the status.

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
                (protect (port/read-line c))
                :caught))))

(ev/join acceptor)
(assert (wait-until (fn [] (and reading (= (fiber/status victim) :paused))))
        "the victim is parked in its read")

# Nothing tells the scheduler the victim is gone: the abort injects an error
# its own `protect` catches, so it runs to :dead with the read still
# submitted, and the regions holding the port and the read buffer are released
# as it unwinds.
(protect (fiber/abort victim {:error :external}))
(assert (= (fiber/status victim) :dead)
        "the victim caught the abort and ran to completion")

# The read is the only operation left, and no byte will ever arrive on it.
# The loop reporting none outstanding is the runtime having ended it; the
# worker count reporting none is that thread coming back.
(defn settled []
  (let [r (ev/report)]
    (and (= 0 (get r :io)) (= 0 (get r :workers)))))

(assert (wait-until settled)
        "the orphaned read ends and gives its worker back, with no peer acting")

(assert (= (ev/join victim) :caught)
        "the victim's value survives its own orphaned read")

(port/close accepted)
(port/close listener)

(println "tests/elle/io-stale-operation-ends.lisp: passed")
