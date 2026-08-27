(elle/epoch 12)
# A PORT completion that arrives after its fiber is gone.
#
# `tests/elle/io-late-completion.lisp` holds the same shape open with
# `ev/sleep`: a fiber parked on an operation is killed by a path the
# scheduler did not route, so the pairing outlives the fiber and the
# completion arrives with no reader. A timer, though, is portless — its
# pending entry owns nothing but a buffer slot, so the completion has
# nothing to read and the drop is free.
#
# A port operation is the case with something at stake. Its entry holds
# values it did not allocate: the port it names and the buffer the caller
# reserved, both born in regions of the fiber that asked. A fiber that
# terminates releases those regions, so a completion assembled from that
# entry afterwards reads freed memory.
#
# The victim below opens its own connection, parks in a read on it,
# catches an abort and runs to `:dead` with the read still submitted, and
# only then does the peer write. Both the port and the read buffer are the
# victim's own, so the completion arrives strictly after the regions that
# hold them are gone.
#
# The trap: this file passes on a plain run either way. The dead port's
# page is recycled by then, so a regression reads a plausible wrong value
# rather than faulting. `io_late_completion_port_uaf`
# (tests/integration/elle_scripts.rs) runs it under `--trace=guardfree`,
# which never re-claims a freed page — that is where the regression is a
# SIGSEGV instead of a false green.
#
# The other trap: the victim is `:paused` in its CONNECT as well as in its
# read, and an abort landing on the connect is caught by no `protect` here
# — it leaves the fiber in `:error` and the file fails with the injected
# payload. So the victim says when it is past the connect, and the wait
# below reads that flag beside the status.

# Bound on how long the victim may take to reach its read, and on the
# peer's bytes reaching the loop. Both are waited for by polling, so
# these only cap a failure.
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

# The peer half, kept out here so it outlives the victim and can write
# after it is gone.
(def @accepted nil)
(def acceptor (ev/spawn (fn [] (assign accepted (tcp/accept listener)))))

(println "a port completion for a fiber that finished reads nothing...")

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

# Nothing tells the scheduler the victim is gone: the abort injects an
# error its own `protect` catches, so it runs to :dead with the read still
# submitted, and the regions holding the port and the read buffer are
# released as it unwinds.
(protect (fiber/abort victim {:error :external}))
(assert (= (fiber/status victim) :dead)
        "the victim caught the abort and ran to completion")

# Now make the completion arrive, with the loop pumping. The victim's read
# is the only operation left, so the loop reporting none outstanding is
# that completion having been taken off the backend.
#
# The write is protected because its success is not the subject: the
# victim's end of this connection went with the victim's regions, so the
# peer may already be writing to a closed socket. Either way the read
# completes and the loop has a completion to take.
(protect (port/write accepted "late\n"))
(assert (wait-until (fn [] (= 0 (get (ev/report) :io))))
        "the late completion is taken off the backend")

(assert (= (ev/join victim) :caught)
        "the victim's value survives its own late port completion")

(port/close accepted)
(port/close listener)

(println "tests/elle/io-late-completion-port.lisp: passed")
