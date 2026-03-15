# Subprocess integration tests
# All I/O-yielding tests run inside ev/spawn.

(def {:assert-eq assert-eq
      :assert-true assert-true
      :assert-false assert-false
      :assert-err assert-err
      :assert-err-kind assert-err-kind}
  ((import-file "tests/elle/assert.lisp")))

# ── process/exec ──────────────────────────────────────────────────────────────

# process/exec: basic struct shape
(let [[proc (ev/spawn (fn [] (process/exec "echo" ["hello"])))]]
  (assert-true (integer? (get proc :pid))          "process/exec: :pid is integer")
  (assert-true (port? (get proc :stdout))          "process/exec: :stdout is port")
  (assert-true (port? (get proc :stderr))          "process/exec: :stderr is port")
  (assert-true (port? (get proc :stdin))           "process/exec: :stdin is port")
  (assert-true (not (nil? (get proc :process)))    "process/exec: :process is set")
  (assert-true (> (get proc :pid) 0)               "process/exec: pid > 0")
  (ev/spawn (fn [] (process/wait proc))))

# process/exec: stdout is binary by default (bytes, not string)
(let [[raw (ev/spawn (fn []
              (let [[proc (process/exec "echo" ["hello"])]]
                (stream/read-all (get proc :stdout)))))]]
  (assert-true (bytes? raw) "process/exec: stdout is bytes"))

# process/exec: decode bytes to string
(assert-eq
  (ev/spawn (fn []
    (let [[proc (process/exec "echo" ["hello"])]]
      (string (stream/read-all (get proc :stdout))))))
  "hello\n"
  "process/exec: stdout bytes decode to string")

# process/exec: binary output (head -c 4 /dev/urandom)
(let [[raw (ev/spawn (fn []
              (let [[proc (process/exec "head" ["-c" "4" "/dev/urandom"])]]
                (stream/read-all (get proc :stdout)))))]]
  (assert-true (bytes? raw) "process/exec: binary output is bytes")
  (assert-eq (length raw) 4 "process/exec: binary output is 4 bytes"))

# process/exec: stdin :null — no stdin pipe
(let [[proc (ev/spawn (fn [] (process/exec "echo" ["hi"] {:stdin :null})))]]
  (assert-true (nil? (get proc :stdin)) "process/exec :stdin :null: stdin is nil")
  (ev/spawn (fn [] (process/wait proc))))

# ── process/wait ──────────────────────────────────────────────────────────────

# process/wait: exit 0
(assert-eq
  (ev/spawn (fn [] (process/wait (process/exec "true" []))))
  0
  "process/wait: /bin/true exits 0")

# process/wait: exit 1
(assert-eq
  (ev/spawn (fn [] (process/wait (process/exec "false" []))))
  1
  "process/wait: /bin/false exits 1")

# process/wait: with direct handle (not struct)
(assert-eq
  (ev/spawn (fn []
    (let [[proc (process/exec "true" [])]]
      (process/wait (get proc :process)))))
  0
  "process/wait: works with direct process handle")

# ── process/pid ───────────────────────────────────────────────────────────────

# process/pid: returns positive integer matching :pid field
(ev/spawn (fn []
  (let [[proc (process/exec "sleep" ["10"])]]
    (assert-true (> (process/pid proc) 0)
                 "process/pid: returns positive integer")
    (assert-eq (process/pid proc) (get proc :pid)
               "process/pid: matches :pid field")
    (process/kill proc 15)
    (process/wait proc))))

# ── process/kill ──────────────────────────────────────────────────────────────

# process/kill: send SIGTERM, wait, exit is nonzero
(let [[exit (ev/spawn (fn []
               (let [[proc (process/exec "sleep" ["60"])]]
                 (process/kill proc 15)
                 (process/wait proc))))]]
  (assert-true (not (= exit 0)) "process/kill: killed process has nonzero exit"))

# process/kill: with explicit signal number 9 (SIGKILL)
(let [[exit (ev/spawn (fn []
               (let [[proc (process/exec "sleep" ["60"])]]
                 (process/kill proc 9)
                 (process/wait proc))))]]
  (assert-true (not (= exit 0)) "process/kill SIGKILL: nonzero exit"))

# process/kill: keyword :sigterm terminates the process
(let [[exit (ev/spawn (fn []
               (let [[proc (process/exec "sleep" ["60"])]]
                 (process/kill proc :sigterm)
                 (process/wait proc))))]]
  (assert-true (not (= exit 0)) "process/kill :sigterm: nonzero exit"))

# ── port/lines with subprocess ────────────────────────────────────────────────

# port/lines on subprocess stdout
(assert-eq
  (ev/spawn (fn []
    (let [[proc (process/exec "printf" ["a\\nb\\nc\\n"])]]
      (stream/collect (port/lines (get proc :stdout))))))
  (list "a" "b" "c")
  "port/lines on subprocess stdout")

# ── stdin write ───────────────────────────────────────────────────────────────

# Write to subprocess stdin, read from stdout
(assert-eq
  (ev/spawn (fn []
    (let [[proc (process/exec "cat" [])]]
      (stream/write (get proc :stdin) "hello from stdin")
      (port/close (get proc :stdin))
      (string (stream/read-all (get proc :stdout))))))
  "hello from stdin"
  "write stdin -> read stdout via cat")

# ── process/system ────────────────────────────────────────────────────────────

# process/system: basic success — exit code
(assert-eq
  (ev/spawn (fn [] (get (process/system "echo" ["hello"]) :exit)))
  0
  "process/system: echo exits 0")

# process/system: stdout captured
(assert-eq
  (ev/spawn (fn [] (get (process/system "echo" ["hello"]) :stdout)))
  "hello\n"
  "process/system: echo stdout")

# process/system: stderr captured and empty
(assert-eq
  (ev/spawn (fn [] (get (process/system "echo" ["hello"]) :stderr)))
  ""
  "process/system: echo stderr is empty")

# process/system: nonzero exit
(let [[result (ev/spawn (fn [] (process/system "false" [])))]]
  (assert-true (not (= (get result :exit) 0))
               "process/system: false has nonzero exit"))

# process/system: result struct shape
(let [[result (ev/spawn (fn [] (process/system "echo" ["test"])))]]
  (assert-true (integer? (get result :exit))   "process/system: :exit is integer")
  (assert-true (string?  (get result :stdout)) "process/system: :stdout is string")
  (assert-true (string?  (get result :stderr)) "process/system: :stderr is string"))

# process/system: concurrent subprocesses
(let [[f1 (ev/spawn (fn [] (get (process/system "echo" ["one"]) :stdout)))]
      [f2 (ev/spawn (fn [] (get (process/system "echo" ["two"]) :stdout)))]]
  (assert-eq f1 "one\n" "concurrent process/system: fiber 1")
  (assert-eq f2 "two\n" "concurrent process/system: fiber 2"))
