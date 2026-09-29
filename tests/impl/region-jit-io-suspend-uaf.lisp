(elle/epoch 13)
# audited: 2026-09-29
# A yielding I/O native called from compiled code retains its escaped request, so the two releases balance.
# docs/impl/region/rules.md
#
# A yielding I/O primitive like `port/read-line` returns `(SIG_IO, IoRequest{buffer})`
# — the IoRequest (an External) and its read buffer (an LBytes) CO-LOCATED in the
# native's one fresh per-execution region (rc=1). The fiber suspends; the
# IoRequest escapes into `fiber.signal`, where the scheduler reads it to perform
# the I/O, and the read buffer becomes the resume RESULT in the same region. So
# that one region is referenced TWICE: by the escaped IoRequest (released by the
# scheduler) and by the result value (released by the consumer's
# `DecrefValueRegion`). Rule 5 ("suspended frame") is the retain that balances
# them: the interpreter's `handle_primitive_signal` (src/vm/signal.rs) takes
# `incref_for_escape(region_of(value), SuspendEscape)`, and its JIT mirror
# `jit_handle_primitive_signal` (src/jit/calls.rs) takes the same.
#
# The counter-factual: a compiled caller without the retain leaves rc at 1, the
# result release frees the region (buffer + IoRequest), and the scheduler's
# release double-frees it — a regionstore phantom/double-free assert, or a
# SIGSEGV under `--trace=guardfree`. The loopback TCP pair below is that shape
# with no network server.
#
# REACHABILITY (why this needs a hot per-call reader, not one big read): the JIT
# only compiles a function once it is HOT (call-counted). `read1` does exactly
# ONE `port/read-line` per call and is called per line, so thousands of calls
# drive it past the adaptive/background-compile threshold; once compiled, every
# subsequent yielding read takes the JIT suspend path. A single function that
# loops the reads internally is called once, never gets hot, and stays
# interpreted and never takes the compiled suspend path. A RESP reader driven hot
# under the async scheduler is the everyday shape.

# ── loopback server: stream a fixed line many times, then close ───────────
(def line-count 20000)
(def the-line "PONGPONGPONG")

(def listener (tcp/listen "127.0.0.1" 0))
(def server-port
  (let [path (port/path listener)]
    (parse-int (slice path (+ 1 (string/find path ":"))))))

(def server
  (ev/spawn (fn []
              (let [client (tcp/accept listener)]
                (var i 0)
                (while (< i line-count)
                  (port/write client (concat the-line "\r\n"))
                  (assign i (+ i 1)))
                (port/flush client)
                (port/close client)))))

# ── the JIT target: ONE yielding read per call, called per line so it goes
#    hot and the JIT compiles it (with a yield side-exit). ──────────────────
(defn read1 (sock)
  (port/read-line sock))

(def reader
  (ev/spawn (fn []
              (let [sock (tcp/connect "127.0.0.1" server-port)]
                (var i 0)
                (var last nil)
                (while (< i line-count)
                  (assign last (read1 sock))
                  (assign i (+ i 1)))
                (port/close sock)
                last))))

# ── witness ───────────────────────────────────────────────────────────────
# Without the retain the hot compiled `read1`'s escaped IoRequest is
# over-released and the program aborts before `reader` finishes. With it every
# read survives and the last line read is intact.
(def result (ev/join-protected reader))
(protect (port/close listener))
(protect (ev/join-protected server))

(assert (get result 0)
        "reader fiber faulted — a JIT yielding-read escape was over-released")
(assert (= (get result 1) the-line)
        "the last JIT-read line was corrupted (its region was freed under the read)")

(println "region-jit-io-suspend-uaf: ok")
