(elle/epoch 12)
## tests/elle/net-wait-timeout.lisp
##
## `:timeout` bounds the socket calls that wait for a peer to appear:
## `tcp/accept`, `unix/accept` and `udp/recv-from`. Each waits on something
## that may never happen — a caller, a datagram — so the deadline is the only
## thing that ends them.
##
## Every case arranges a peer that acts 3 seconds in rather than leaving the
## call to wait forever. That bound is what makes a failure legible: an
## unbounded accept does not hang the suite, it returns that late peer's
## connection about 3 seconds in. So `elapsed` is the real discriminator, and
## the error kind pins that the call ended for the right reason.
##
## `tcp/connect` and `unix/connect` belong to this family and take the same
## bound. Stalling one needs a listener backlog no Elle primitive sets, so they
## are pinned in src/io/aio/tests/net.rs instead.
##
## The thread-pool backend runs this file via the `net_wait_timeout_threadpool`
## pin in tests/integration/elle_scripts.rs (`--no-uring` is process-global).

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn timed [thunk]
  "Run `thunk` under protect. Returns [ok? result elapsed-seconds]."
  (let* [started (clock/monotonic)
         [ok? result] (protect (thunk))
         elapsed (- (clock/monotonic) started)]
    [ok? result elapsed]))

(defn assert-timed-out [label outcome]
  "Assert the call ended at its 500 ms deadline with a :timeout error."
  (let [[ok? result elapsed] outcome]
    ## Checked first: an unbounded call runs until the late peer acts, so this
    ## separates "stopped at the deadline" from "stopped because a peer showed
    ## up".
    (assert (< elapsed 2)
            (concat label ": ran " (string elapsed)
                    "s against a :timeout 500 — the call waited for a peer"
                    " instead of its own deadline"))
    (assert (not ok?)
            (concat label ": a peer that never comes must signal, got "
                    (string result)))
    (assert (= (get result :error) :timeout)
            (concat label ": expected a :timeout error, got " (string result)))))

## ── 1. tcp/accept on a listener nobody calls ─────────────────────────

(defn tcp-accept-outcome []
  "Accept on a listener whose only caller arrives long after the deadline."
  (ev/run (fn []
            (let* [listener (tcp/listen "127.0.0.1" 0)
                   port-num (listen-port listener)]
              ## The late peer. Its connect fails once the deadline has closed
              ## the listener, which is the healthy outcome and not this
              ## test's subject — hence the protect.
              (ev/spawn (fn []
                          (ev/sleep 3)
                          (protect (port/close (tcp/connect "127.0.0.1" port-num
                                   :timeout 1000)))))
              (let [outcome (timed (fn [] (tcp/accept listener :timeout 500)))]
                (port/close listener)
                outcome)))))

(assert-timed-out "tcp/accept" (tcp-accept-outcome))

(println "  1. tcp/accept stops at its deadline with no caller")

## ── 2. unix/accept on a listener nobody calls ────────────────────────
##
## The same path as case 1 — one `PortOp::Accept` serves both socket families
## — pinned on the other family so a kind-specific regression cannot hide.

(defn unix-accept-outcome [dir]
  "The case-1 shape on a Unix listener. `dir` holds the socket."
  ## Short basename: sun_path is capped at 108 bytes.
  (let [sock (path/join dir "w.sock")]
    (ev/run (fn []
              (let [listener (unix/listen sock)]
                (ev/spawn (fn []
                            (ev/sleep 3)
                            (protect (port/close (unix/connect sock
                                     :timeout 1000)))))
                (let [outcome (timed (fn [] (unix/accept listener :timeout 500)))]
                  (port/close listener)
                  outcome))))))

(with-temp-dir d (assert-timed-out "unix/accept" (unix-accept-outcome d)))

(println "  2. unix/accept stops at its deadline with no caller")

## ── 3. udp/recv-from on a socket nobody sends to ─────────────────────

(defn recv-from-outcome []
  "Receive on a socket whose only datagram is sent long after the deadline."
  (ev/run (fn []
            (let* [sock (udp/bind "127.0.0.1" 0)
                   port-num (listen-port sock)]
              (ev/spawn (fn []
                          (ev/sleep 3)
                          (protect (udp/send-to sock "late" "127.0.0.1" port-num
                                   :timeout 1000))))
              (let [outcome (timed (fn [] (udp/recv-from sock 64 :timeout 500)))]
                (port/close sock)
                outcome)))))

(assert-timed-out "udp/recv-from" (recv-from-outcome))

(println "  3. udp/recv-from stops at its deadline with no sender")

(println "net-wait-timeout: all tests passed")
