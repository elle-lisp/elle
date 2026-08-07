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

## Every step of a case announces itself before it runs, so a run that does not
## finish names the call it stopped in. A case that completes prints its own
## line to stdout; these go to stderr, and only the last one printed matters.
(defn step [label]
  (eprintln "    · " label))

(ev/run (fn []
          (let* [_ (step "1: listen")
                 listener (tcp/listen "127.0.0.1" 0)
                 port-num (listen-port listener)
                 @peer nil]
            ## Accept, then hold the connection open without ever reading from it.
            (ev/spawn (fn [] (assign peer (tcp/accept listener))))
            (step "1: connect")
            (let* [conn (tcp/connect "127.0.0.1" port-num :sndbuf 4096
                                     :timeout 5000)
                   _ (step "1: build an 8 MB payload")
                   payload (bytes (string/repeat "x" 8000000))
                   _ (step "1: write it with :timeout 500")
                   started (clock/monotonic)
                   [ok? err] (protect (port/write conn payload :timeout 500))
                   elapsed (- (clock/monotonic) started)]
              (step (concat "1: the write returned after " (string elapsed)
                            "s, ok?=" (string ok?) " err=" (string err)))
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
##
## The peer's pacing only gates the call if the payload cannot simply sit in
## the kernel's buffers: a write the buffers swallow whole returns before the
## peer has read anything, and this case would then be measuring nothing. How
## much they swallow is a per-platform number and `:sndbuf` is a request, not a
## setting — macOS caps a socket's buffers at 8 MiB by default. So the payload
## is 12 MiB, above any of them, and the several megabytes that cannot fit have
## to cross at the peer's pace against a 1000 ms per-operation deadline.
##
## The peer paces with `port/read-exact`, not `port/read`: `port/read` returns
## up to what was asked for, so the rate it sets is whatever the kernel happens
## to hand over per call, and the case's runtime would follow that rather than
## the sleep. `read-exact` consumes the same 256 KiB per tick on any backend,
## which keeps this bounded well inside the corpus budget.

(def slow-peer-payload (* 12 1024 1024))
(def slow-peer-chunk 262144)

(ev/run (fn []
          (let* [_ (step "2: listen")
                 listener (tcp/listen "127.0.0.1" 0)
                 port-num (listen-port listener)]
            (ev/spawn (fn []
                        (let [conn (tcp/accept listener)]
                          (forever
                            (let [chunk (port/read-exact conn slow-peer-chunk)]
                              (when (nil? chunk) (break))
                              (ev/sleep 0.1)))
                          (port/close conn))))
            (step "2: connect")
            (let* [conn (tcp/connect "127.0.0.1" port-num :sndbuf 4096
                                     :timeout 5000)
                   _ (step "2: write 12 MiB to a peer that reads slowly")
                   started (clock/monotonic)
                   returned (port/write conn
                                        (bytes (string/repeat "x"
                                        slow-peer-payload)) :timeout 1000)
                   elapsed (- (clock/monotonic) started)]
              (step (concat "2: the write returned after " (string elapsed)
                            "s, returned=" (string returned)))
              (assert (= returned slow-peer-payload)
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
          (let* [_ (step "3: spawn a child that never reads its stdin")
                 child (subprocess/exec "sleep" ["4"])
                 _ (step "3: write 1 MB into its stdin with :timeout 500")
                 started (clock/monotonic)
                 [ok? err] (protect (port/write (get child :stdin)
                                    (bytes (string/repeat "x" 1000000))
                                    :timeout 500))
                 elapsed (- (clock/monotonic) started)
                 _ (step "3: the write returned")]
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
