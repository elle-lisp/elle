# Trace output tests
#
# Tests that vm/config trace keywords produce [trace:...] output on stderr,
# and that trace output is absent when keywords are not set.

# ============================================================================
# Trace output format
# ============================================================================

# Enable :call tracing, call a function, verify output appeared.
# We capture stderr by running a subprocess.

(defn run-with-trace [trace-kw code]
  "Run elle code with a trace keyword and return stderr."
  (let* [[[cmd (string "echo '" code "' | " (sys/argv) " --trace=" trace-kw " -")]]
         [[result (subprocess/exec "sh" "-c" cmd)]]]
    (get result :stderr)))

# :call trace produces [trace:call] output
(let [[[stderr (run-with-trace "call" "(defn f [x] (+ x 1)) (f 42)")]]
  (assert (string/find stderr "[trace:call]")
    "trace=call produces [trace:call] output"))

# :signal trace produces [trace:signal] output — trigger a signal by doing I/O
(let [[[stderr (run-with-trace "signal" "(display 42)")]]
  (assert (string/find stderr "[trace:signal]")
    "trace=signal produces [trace:signal] output"))

# ============================================================================
# No trace output without flag
# ============================================================================

(let* [[[cmd (string "echo '(defn f [x] (+ x 1)) (f 42)' | " (sys/argv) " -")]]
       [[result (subprocess/exec "sh" "-c" cmd)]]
       [[stderr (get result :stderr)]]]
  (assert (not (string/find stderr "[trace:"))
    "no trace output without --trace flag"))

# ============================================================================
# Elle-level trace enable mid-program
# ============================================================================

# Enable tracing from Elle code, then call a function
(let* [[[code "(put (vm/config) :trace |:call|) (defn g [x] (* x 2)) (g 7)"]]
       [[cmd (string "echo '" code "' | " (sys/argv) " -")]]
       [[result (subprocess/exec "sh" "-c" cmd)]]
       [[stderr (get result :stderr)]]]
  (assert (string/find stderr "[trace:call]")
    "Elle-level trace enable produces [trace:call] output"))

# ============================================================================
# Multiple trace keywords
# ============================================================================

(let [[[stderr (run-with-trace "call,signal" "(defn f [x] (+ x 1)) (f 42) (display 1)")]]
  (assert (string/find stderr "[trace:call]")
    "multiple traces: call output present")
  (assert (string/find stderr "[trace:signal]")
    "multiple traces: signal output present"))

# ============================================================================
# --trace=all enables everything
# ============================================================================

(let [[[stderr (run-with-trace "all" "(defn f [x] (+ x 1)) (f 42)")]]
  (assert (string/find stderr "[trace:")
    "trace=all produces some trace output"))
