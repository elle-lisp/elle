(elle/epoch 12)
## tests/elle/port-read-timeout.lisp
##
## `:timeout` bounds a read that needs more than one kernel operation, and it
## bounds each operation rather than the whole call.
##
## `port/read` is a single "up to n bytes" operation. The other three loop:
## `port/read-exact` resubmits until it has the full count, `port/read-all`
## until EOF, and `port/read-line` until a newline arrives. A peer that
## delivers part of the data and then goes quiet leaves each of them waiting on
## a resubmission, so the deadline has to ride every operation, not just the
## first.
##
## Each case below holds the connection open for 5 seconds and then closes it,
## rather than stalling forever. That bound is what makes the failure legible:
## an unbounded read does not hang the suite, it returns whatever the close
## produced — nil for `read-exact`, the partial for `read-all`/`read-line` —
## about 5 seconds in. So `elapsed` is the real discriminator, and the error
## kind pins that the call ended for the right reason.
##
## Case 5 is the opposite direction. A deadline for the whole call would also
## satisfy cases 1-4 and also report `:timeout`, while breaking every healthy
## transfer from a peer that is merely slow. Only a per-operation deadline
## satisfies both.
##
## `port-write-timeout.lisp` is the write-direction twin.
## The thread-pool backend runs this file via the `port_read_timeout_threadpool`
## pin in tests/integration/elle_scripts.rs (`--no-uring` is process-global).

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(defn from-stalled-peer [prelude body]
  "Serve `prelude` bytes, go quiet, then close after 5s. Runs `body` on the
   client. Returns [ok? result elapsed]."
  (ev/run (fn []
            (let* [listener (tcp/listen "127.0.0.1" 0)
                   port-num (listen-port listener)]
              (ev/spawn (fn []
                          (let [conn (tcp/accept listener)]
                            (when (> prelude 0)
                              (port/write conn
                              (bytes (string/repeat "y" prelude))))
                            ## Quiet, but still open: the reader waits on data, not EOF.
                            (ev/sleep 5)
                            (port/close conn))))
              (let* [conn (tcp/connect "127.0.0.1" port-num :timeout 5000)
                     started (clock/monotonic)
                     [ok? result] (protect (body conn))
                     elapsed (- (clock/monotonic) started)]
                (port/close conn)
                (port/close listener)
                [ok? result elapsed])))))

(defn assert-timed-out [label outcome]
  "Assert the call ended at its 500 ms deadline with a :timeout error."
  (let [[ok? result elapsed] outcome]
    ## Checked first: an unbounded read runs until the peer's 5s close, so this
    ## separates "stopped at the deadline" from "stopped because the peer left".
    (assert (< elapsed 2)
            (concat label ": ran " (string elapsed)
                    "s against a :timeout 500 — the read waited for the peer's"
                    " close instead of its own deadline"))
    (assert (not ok?)
            (concat label ": a stalled peer must signal, got " (string result)))
    (assert (= (get result :error) :timeout)
            (concat label ": expected a :timeout error, got " (string result)))))

## ── 1-3. The looping reads, stalled mid-operation ────────────────────

(assert-timed-out "read-exact"
                  (from-stalled-peer 10
                                     (fn [conn]
                                       (port/read-exact conn 1000 :timeout 500))))

(println "  1. read-exact stops at its deadline mid-count")

(assert-timed-out "read-all"
                  (from-stalled-peer 10
                                     (fn [conn]
                                       (port/read-all conn :timeout 500))))

(println "  2. read-all stops at its deadline before EOF")

(assert-timed-out "read-line"
                  (from-stalled-peer 10
                                     (fn [conn]
                                       (port/read-line conn :timeout 500))))

(println "  3. read-line stops at its deadline mid-line")

## ── 4. The single-operation read, against a peer that sends nothing ──
##
## One kernel operation, so this needs no re-arming — but the thread-pool
## backend bounds it by the same mechanism as the looping reads, and had no
## bound at all before.

(assert-timed-out "read"
                  (from-stalled-peer 0
                                     (fn [conn]
                                       (port/read conn 1000 :timeout 500))))

(println "  4. read stops at its deadline against a silent peer")

## ── 5. A peer that is slow but always progressing ────────────────────
##
## Every gap stays inside the timeout while the whole call runs well past it.
## This is what distinguishes a per-operation deadline from a per-call one.

(ev/run (fn []
          (let* [listener (tcp/listen "127.0.0.1" 0)
                 port-num (listen-port listener)
                 chunk 4096
                 chunks 10]
            (ev/spawn (fn []
                        (let [conn (tcp/accept listener)]
                          (repeat chunks
                                  (port/write conn
                                  (bytes (string/repeat "z" chunk)))
                                  (ev/sleep 0.15))
                          (ev/sleep 3)
                          (port/close conn))))
            (let* [conn (tcp/connect "127.0.0.1" port-num :timeout 5000)
                   want (* chunk chunks)
                   started (clock/monotonic)
                   got (port/read-exact conn want :timeout 1000)
                   elapsed (- (clock/monotonic) started)]
              (assert (= (length got) want)
                      (concat "a slow peer must still deliver every byte, got "
                              (string (length got)) " of " (string want)))
              (assert (> elapsed 1.0)
                      (concat "the call ran " (string elapsed)
                              "s, inside its :timeout 1000 — the peer's pacing should"
                              " have carried it past the deadline"))
              (port/close conn)
              (port/close listener)))))

(println "  5. a slow but progressing peer does not trip the timeout")

(println "port-read-timeout: all tests passed")
