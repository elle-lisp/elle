# Subprocess integration tests
# All I/O-yielding tests run inside ev/spawn.

(def {:assert-eq assert-eq
      :assert-true assert-true
      :assert-false assert-false
      :assert-err assert-err
      :assert-err-kind assert-err-kind}
  ((import-file "tests/elle/assert.lisp")))

# ── sys/exec ──────────────────────────────────────────────────────────────────

# sys/exec: basic struct shape
(let [[proc (ev/spawn (fn [] (sys/exec "echo" ["hello"])))]]
  (assert-true (integer? (get proc :pid))          "sys/exec: :pid is integer")
  (assert-true (port? (get proc :stdout))          "sys/exec: :stdout is port")
  (assert-true (port? (get proc :stderr))          "sys/exec: :stderr is port")
  (assert-true (port? (get proc :stdin))           "sys/exec: :stdin is port")
  (assert-true (not (nil? (get proc :process)))    "sys/exec: :process is set")
  (assert-true (> (get proc :pid) 0)               "sys/exec: pid > 0")
  (ev/spawn (fn [] (sys/wait proc))))

# sys/exec: stdout is binary by default (bytes, not string)
(let [[raw (ev/spawn (fn []
              (let [[proc (sys/exec "echo" ["hello"])]]
                (stream/read-all (get proc :stdout)))))]]
  (assert-true (bytes? raw) "sys/exec: stdout is bytes"))

# sys/exec: decode bytes to string
(assert-eq
  (ev/spawn (fn []
    (let [[proc (sys/exec "echo" ["hello"])]]
      (string (stream/read-all (get proc :stdout))))))
  "hello\n"
  "sys/exec: stdout bytes decode to string")

# sys/exec: binary output (head -c 4 /dev/urandom)
(let [[raw (ev/spawn (fn []
              (let [[proc (sys/exec "head" ["-c" "4" "/dev/urandom"])]]
                (stream/read-all (get proc :stdout)))))]]
  (assert-true (bytes? raw) "sys/exec: binary output is bytes")
  (assert-eq (length raw) 4 "sys/exec: binary output is 4 bytes"))

# sys/exec: stdin :null — no stdin pipe
(let [[proc (ev/spawn (fn [] (sys/exec "echo" ["hi"] {:stdin :null})))]]
  (assert-true (nil? (get proc :stdin)) "sys/exec :stdin :null: stdin is nil")
  (ev/spawn (fn [] (sys/wait proc))))

# ── sys/wait ──────────────────────────────────────────────────────────────────

# sys/wait: exit 0
(assert-eq
  (ev/spawn (fn [] (sys/wait (sys/exec "true" []))))
  0
  "sys/wait: /bin/true exits 0")

# sys/wait: exit 1
(assert-eq
  (ev/spawn (fn [] (sys/wait (sys/exec "false" []))))
  1
  "sys/wait: /bin/false exits 1")

# sys/wait: with direct handle (not struct)
(assert-eq
  (ev/spawn (fn []
    (let [[proc (sys/exec "true" [])]]
      (sys/wait (get proc :process)))))
  0
  "sys/wait: works with direct process handle")

# ── process/pid ───────────────────────────────────────────────────────────────

# process/pid: returns positive integer matching :pid field
(ev/spawn (fn []
  (let [[proc (sys/exec "sleep" ["10"])]]
    (assert-true (> (process/pid proc) 0)
                 "process/pid: returns positive integer")
    (assert-eq (process/pid proc) (get proc :pid)
               "process/pid: matches :pid field")
    (sys/kill proc 15)
    (sys/wait proc))))

# ── sys/kill ──────────────────────────────────────────────────────────────────

# sys/kill: send SIGTERM, wait, exit is nonzero
(let [[exit (ev/spawn (fn []
               (let [[proc (sys/exec "sleep" ["60"])]]
                 (sys/kill proc 15)
                 (sys/wait proc))))]]
  (assert-true (not (= exit 0)) "sys/kill: killed process has nonzero exit"))

# sys/kill: with explicit signal number 9 (SIGKILL)
(let [[exit (ev/spawn (fn []
               (let [[proc (sys/exec "sleep" ["60"])]]
                 (sys/kill proc 9)
                 (sys/wait proc))))]]
  (assert-true (not (= exit 0)) "sys/kill SIGKILL: nonzero exit"))

# ── port/lines with subprocess ────────────────────────────────────────────────

# port/lines on subprocess stdout
(assert-eq
  (ev/spawn (fn []
    (let [[proc (sys/exec "printf" ["a\\nb\\nc\\n"])]]
      (stream/collect (port/lines (get proc :stdout))))))
  (list "a" "b" "c")
  "port/lines on subprocess stdout")

# ── stdin write ───────────────────────────────────────────────────────────────

# Write to subprocess stdin, read from stdout
(assert-eq
  (ev/spawn (fn []
    (let [[proc (sys/exec "cat" [])]]
      (stream/write (get proc :stdin) "hello from stdin")
      (port/close (get proc :stdin))
      (string (stream/read-all (get proc :stdout))))))
  "hello from stdin"
  "write stdin -> read stdout via cat")

# ── sys/system ────────────────────────────────────────────────────────────────

# sys/system: basic success — exit code
(assert-eq
  (ev/spawn (fn [] (get (sys/system "echo" ["hello"]) :exit)))
  0
  "sys/system: echo exits 0")

# sys/system: stdout captured
(assert-eq
  (ev/spawn (fn [] (get (sys/system "echo" ["hello"]) :stdout)))
  "hello\n"
  "sys/system: echo stdout")

# sys/system: stderr captured and empty
(assert-eq
  (ev/spawn (fn [] (get (sys/system "echo" ["hello"]) :stderr)))
  ""
  "sys/system: echo stderr is empty")

# sys/system: nonzero exit
(let [[result (ev/spawn (fn [] (sys/system "false" [])))]]
  (assert-true (not (= (get result :exit) 0))
               "sys/system: false has nonzero exit"))

# sys/system: result struct shape
(let [[result (ev/spawn (fn [] (sys/system "echo" ["test"])))]]
  (assert-true (integer? (get result :exit))   "sys/system: :exit is integer")
  (assert-true (string?  (get result :stdout)) "sys/system: :stdout is string")
  (assert-true (string?  (get result :stderr)) "sys/system: :stderr is string"))

# sys/system: concurrent subprocesses
(let [[f1 (ev/spawn (fn [] (get (sys/system "echo" ["one"]) :stdout)))]
      [f2 (ev/spawn (fn [] (get (sys/system "echo" ["two"]) :stdout)))]]
  (assert-eq f1 "one\n" "concurrent sys/system: fiber 1")
  (assert-eq f2 "two\n" "concurrent sys/system: fiber 2"))
