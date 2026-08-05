(elle/epoch 12)
## tests/elle/port-timeout-shared-fd.lisp
##
## Two timed operations on one descriptor each keep their own deadline.
##
## A port is duplex: a read and a write can be in flight on it at the same
## time, on the same descriptor. The thread-pool backend bounds an operation by
## taking the descriptor non-blocking for that operation's lifetime, and the
## non-blocking flag belongs to the descriptor rather than to the operation —
## so the two operations share it. Whichever finishes first must leave the flag
## alone while the other still runs; putting the descriptor back to blocking
## there parks the survivor in the kernel with no bound at all.
##
## The order is what makes the shared state visible. The read never receives a
## byte and gives up first, at 200 ms. The write is still going when it does —
## the peer reads for a full second before it stops — so the write issues
## kernel operations after the read has finished with the descriptor. Only then
## does the peer stall, and the write must still trip its own 500 ms deadline.
##
## The peer closes at 4 s rather than holding forever, so a lost bound returns
## late instead of hanging: `elapsed` then names which happened.
##
## The thread-pool backend runs this file via the
## `port_timeout_shared_fd_threadpool` pin in tests/integration/elle_scripts.rs
## (`--no-uring` is process-global).

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(ev/run (fn []
          (let* [listener (tcp/listen "127.0.0.1" 0)
                 port-num (listen-port listener)]
            ## Accept, drain for a second, then stall while holding the
            ## connection open. The peer never sends anything of its own.
            (ev/spawn (fn []
                        (let [conn (tcp/accept listener)]
                          (repeat 10 (port/read conn 32768) (ev/sleep 0.1))
                          (ev/sleep 3)
                          (port/close conn))))
            (let* [conn (tcp/connect "127.0.0.1" port-num :sndbuf 4096
                                     :timeout 5000)
                   reader (ev/spawn (fn []
                                      (let* [started (clock/monotonic)
                                        [ok? result] (protect (port/read conn
                                        1000 :timeout 200))]
                                        [ok? result
                                        (- (clock/monotonic) started)])))]
              ## Let the read reach the kernel before the write joins it on the
              ## same descriptor. Both are then in flight together, which is
              ## the state under test.
              (ev/sleep 0.05)
              (let* [started (clock/monotonic)
                     [wok? werr] (protect (port/write conn
                     (bytes (string/repeat "x" 8000000)) :timeout 500))
                     welapsed (- (clock/monotonic) started)
                     [rok? rresult relapsed] (ev/join reader)]
                (assert (< welapsed 3)
                        (concat "the write ran " (string welapsed)
                                "s against a :timeout 500 — it waited for the"
                                " peer's close, so the read that shared the"
                                " descriptor took its bound with it"))
                (assert (< relapsed 3)
                        (concat "the read ran " (string relapsed)
                                "s against a :timeout 200 — the write that"
                                " shared the descriptor took its bound with it"))
                (assert (not wok?)
                        "a peer that never reads must trip the write's deadline")
                (assert (= (get werr :error) :timeout)
                        (concat "expected a :timeout error from the write, got "
                                (string werr)))
                (assert (not rok?)
                        (concat "a peer that never writes must trip the read's"
                                " deadline, got " (string rresult)))
                (assert (= (get rresult :error) :timeout)
                        (concat "expected a :timeout error from the read, got "
                                (string rresult)))
                (port/close conn)
                (port/close listener))))))

(println "port-timeout-shared-fd: both operations kept their deadline")
