(elle/epoch 12)
## tests/elle/port-write-timeout.lisp
##
## `port/write`'s `:timeout` bounds the call even when the payload needs more
## than one syscall to leave.
##
## `port/write` writes every byte before it returns (docs/io.md), so a payload
## larger than the fd's send buffer completes over several kernel operations.
## Each of them carries the caller's timeout. Without that, only the first
## operation is bounded and the rest wait forever, so a peer that stops reading
## hangs a write that explicitly asked not to hang — the failure mode
## `port-shortwrite.lisp`'s truncation was reported as.
##
## The peer here accepts the connection and never reads a byte. With a 4 KiB
## send buffer, the first operation takes a few kilobytes of the payload and
## every later one waits on a socket that never drains. The write must give up
## at its deadline and signal, not block until the corpus runner's file
## timeout kills it.

(defn listen-port [listener]
  "Return the port number of a listener bound to an ephemeral port."
  (let [parts (string/split (port/path listener) ":")]
    (parse-int (get parts (- (length parts) 1)))))

(ev/run (fn []
          (let* [listener (tcp/listen "127.0.0.1" 0)
                 port-num (listen-port listener)
                 @peer nil]
            ## Accept, then hold the connection open without ever reading from it.
            (ev/spawn (fn [] (assign peer (tcp/accept listener))))
            (let* [conn (tcp/connect "127.0.0.1" port-num :sndbuf 4096
                                     :timeout 5000)
                   started (clock/monotonic)
                   [ok? err] (protect (port/write conn
                                      (bytes (string/repeat "x" 8000000))
                                      :timeout 500))
                   elapsed (- (clock/monotonic) started)]
              (assert (not ok?)
                      "port/write to a peer that never reads must signal, not succeed")
              (assert (= (get err :error) :timeout)
                      (concat "expected a :timeout error, got " (string err)))
              ## Generous ceiling: the assertion is "it returns at its deadline", not a
              ## latency measurement. A regression that drops the re-armed timeout does
              ## not return at all.
              (assert (< elapsed 10)
                      (concat "port/write ran " (string elapsed)
                              "s past a :timeout 500 — the resubmitted chunks are"
                              " unbounded"))
              (port/close conn)
              (when peer (port/close peer))
              (port/close listener)))))

(println "  1. :timeout bounds a write that outgrows one syscall")

## The other direction: the timeout bounds each operation, not the call. A peer
## that reads slowly keeps every operation inside the deadline while the whole
## write takes far longer than it. Re-arming the timeout per operation is what
## makes both properties hold at once.
##
## This case exists because case 1 alone does not constrain the fix. Treating
## `:timeout` as a deadline for the entire call also stops the hang, and also
## reports `:timeout` — and breaks every healthy transfer to a slow reader.
## Here the peer reads 32 KiB every 150 ms, so the payload needs more than ten
## reads and the call cannot finish inside its 1000 ms timeout.

(ev/run (fn []
          (let* [listener (tcp/listen "127.0.0.1" 0)
                 port-num (listen-port listener)]
            (ev/spawn (fn []
                        (let [conn (tcp/accept listener)]
                          (forever
                            (let [chunk (port/read conn 32768)]
                              (when (nil? chunk) (break))
                              (ev/sleep 0.15)))
                          (port/close conn))))
            (let* [conn (tcp/connect "127.0.0.1" port-num :sndbuf 4096
                                     :timeout 5000)
                   started (clock/monotonic)
                   returned (port/write conn (bytes (string/repeat "x" 400000))
                                        :timeout 1000)
                   elapsed (- (clock/monotonic) started)]
              (assert (= returned 400000)
                      (concat "a slow peer must still receive every byte, returned "
                              (string returned)))
              (assert (> elapsed 1.0)
                      (concat "the call ran " (string elapsed)
                              "s, inside its :timeout 1000 — the peer's pacing should"
                              " have carried it past the deadline"))
              (port/close conn)
              (port/close listener)))))

(println "  2. a slow but progressing peer does not trip the timeout")

## A pipe stalls on an absent reader exactly as a socket does, and it rejects
## the socket options that bound a socket. So the bound has to belong to the
## operation rather than to the descriptor: the child below never reads its
## stdin, the pipe buffer fills, and the rest of the payload has nowhere to go.
##
## The child exits after 4 s and its end of the pipe closes with it, so an
## unbounded write does not hang the suite — it returns about 4 s in with the
## EPIPE that close produced. `elapsed` separates the two: a bounded write
## returns at its own 500 ms deadline, and the error kind says it ended for its
## deadline rather than for the peer's exit.

(ev/run (fn []
          (let* [child (subprocess/exec "sleep" ["4"])
                 started (clock/monotonic)
                 [ok? err] (protect (port/write (get child :stdin)
                                    (bytes (string/repeat "x" 1000000))
                                    :timeout 500))
                 elapsed (- (clock/monotonic) started)]
            (protect (subprocess/kill child :sigterm))
            (protect (subprocess/wait child))
            (assert (< elapsed 2)
                    (concat "port/write to a pipe ran " (string elapsed)
                            "s against a :timeout 500 — the write waited for the"
                            " child's exit instead of its own deadline"))
            (assert (not ok?)
                    "port/write to a child that never reads must signal, not succeed")
            (assert (= (get err :error) :timeout)
                    (concat "expected a :timeout error from the pipe write, got "
                            (string err))))))

(println "  3. :timeout bounds a write to a pipe nobody reads")

(println "port-write-timeout: all tests passed")
