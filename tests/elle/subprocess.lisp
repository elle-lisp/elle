# Subprocess integration tests
# All I/O-yielding tests run inside ev/spawn.

(def {:assert-eq assert-eq
      :assert-true assert-true
      :assert-false assert-false
      :assert-err assert-err
      :assert-err-kind assert-err-kind}
  ((import-file "tests/elle/assert.lisp")))

# ── subprocess/exec ──────────────────────────────────────────────────────────────

# subprocess/exec: basic struct shape
(let [[proc (ev/spawn (fn [] (subprocess/exec "echo" ["hello"])))]]
  (assert-true (integer? (get proc :pid))          "subprocess/exec: :pid is integer")
  (assert-true (port? (get proc :stdout))          "subprocess/exec: :stdout is port")
  (assert-true (port? (get proc :stderr))          "subprocess/exec: :stderr is port")
  (assert-true (port? (get proc :stdin))           "subprocess/exec: :stdin is port")
  (assert-true (not (nil? (get proc :process)))    "subprocess/exec: :process is set")
  (assert-true (> (get proc :pid) 0)               "subprocess/exec: pid > 0")
  (ev/spawn (fn [] (subprocess/wait proc))))

# subprocess/exec: stdout is binary by default (bytes, not string)
(let [[raw (ev/spawn (fn []
              (let [[proc (subprocess/exec "echo" ["hello"])]]
                (stream/read-all (get proc :stdout)))))]]
  (assert-true (bytes? raw) "subprocess/exec: stdout is bytes"))

# subprocess/exec: decode bytes to string
(assert-eq
  (ev/spawn (fn []
    (let [[proc (subprocess/exec "echo" ["hello"])]]
      (string (stream/read-all (get proc :stdout))))))
  "hello\n"
  "subprocess/exec: stdout bytes decode to string")

# subprocess/exec: binary output (head -c 4 /dev/urandom)
(let [[raw (ev/spawn (fn []
              (let [[proc (subprocess/exec "head" ["-c" "4" "/dev/urandom"])]]
                (stream/read-all (get proc :stdout)))))]]
  (assert-true (bytes? raw) "subprocess/exec: binary output is bytes")
  (assert-eq (length raw) 4 "subprocess/exec: binary output is 4 bytes"))

# subprocess/exec: stdin :null — no stdin pipe
(let [[proc (ev/spawn (fn [] (subprocess/exec "echo" ["hi"] {:stdin :null})))]]
  (assert-true (nil? (get proc :stdin)) "subprocess/exec :stdin :null: stdin is nil")
  (ev/spawn (fn [] (subprocess/wait proc))))

# ── subprocess/wait ──────────────────────────────────────────────────────────────

# subprocess/wait: exit 0
(assert-eq
  (ev/spawn (fn [] (subprocess/wait (subprocess/exec "true" []))))
  0
  "subprocess/wait: /bin/true exits 0")

# subprocess/wait: exit 1
(assert-eq
  (ev/spawn (fn [] (subprocess/wait (subprocess/exec "false" []))))
  1
  "subprocess/wait: /bin/false exits 1")

# subprocess/wait: with direct handle (not struct)
(assert-eq
  (ev/spawn (fn []
    (let [[proc (subprocess/exec "true" [])]]
      (subprocess/wait (get proc :process)))))
  0
  "subprocess/wait: works with direct process handle")

# ── subprocess/pid ───────────────────────────────────────────────────────────────

# subprocess/pid: returns positive integer matching :pid field
(ev/spawn (fn []
  (let [[proc (subprocess/exec "sleep" ["10"])]]
    (assert-true (> (subprocess/pid proc) 0)
                 "subprocess/pid: returns positive integer")
    (assert-eq (subprocess/pid proc) (get proc :pid)
               "subprocess/pid: matches :pid field")
    (subprocess/kill proc 15)
    (subprocess/wait proc))))

# ── subprocess/kill ──────────────────────────────────────────────────────────────

# subprocess/kill: send SIGTERM, wait, exit is nonzero
(let [[exit (ev/spawn (fn []
               (let [[proc (subprocess/exec "sleep" ["60"])]]
                 (subprocess/kill proc 15)
                 (subprocess/wait proc))))]]
  (assert-true (not (= exit 0)) "subprocess/kill: killed process has nonzero exit"))

# subprocess/kill: with explicit signal number 9 (SIGKILL)
(let [[exit (ev/spawn (fn []
               (let [[proc (subprocess/exec "sleep" ["60"])]]
                 (subprocess/kill proc 9)
                 (subprocess/wait proc))))]]
  (assert-true (not (= exit 0)) "subprocess/kill SIGKILL: nonzero exit"))

# subprocess/kill: keyword :sigterm terminates the process
(let [[exit (ev/spawn (fn []
               (let [[proc (subprocess/exec "sleep" ["60"])]]
                 (subprocess/kill proc :sigterm)
                 (subprocess/wait proc))))]]
  (assert-true (not (= exit 0)) "subprocess/kill :sigterm: nonzero exit"))

# ── port/lines with subprocess ────────────────────────────────────────────────

# port/lines on subprocess stdout
(assert-eq
  (ev/spawn (fn []
    (let [[proc (subprocess/exec "printf" ["a\\nb\\nc\\n"])]]
      (stream/collect (port/lines (get proc :stdout))))))
  (list "a" "b" "c")
  "port/lines on subprocess stdout")

# ── stdin write ───────────────────────────────────────────────────────────────

# Write to subprocess stdin, read from stdout
(assert-eq
  (ev/spawn (fn []
    (let [[proc (subprocess/exec "cat" [])]]
      (stream/write (get proc :stdin) "hello from stdin")
      (port/close (get proc :stdin))
      (string (stream/read-all (get proc :stdout))))))
  "hello from stdin"
  "write stdin -> read stdout via cat")

# ── subprocess/system ────────────────────────────────────────────────────────────

# subprocess/system: basic success — exit code
(assert-eq
  (ev/spawn (fn [] (get (subprocess/system "echo" ["hello"]) :exit)))
  0
  "subprocess/system: echo exits 0")

# subprocess/system: stdout captured
(assert-eq
  (ev/spawn (fn [] (get (subprocess/system "echo" ["hello"]) :stdout)))
  "hello\n"
  "subprocess/system: echo stdout")

# subprocess/system: stderr captured and empty
(assert-eq
  (ev/spawn (fn [] (get (subprocess/system "echo" ["hello"]) :stderr)))
  ""
  "subprocess/system: echo stderr is empty")

# subprocess/system: nonzero exit
(let [[result (ev/spawn (fn [] (subprocess/system "false" [])))]]
  (assert-true (not (= (get result :exit) 0))
               "subprocess/system: false has nonzero exit"))

# subprocess/system: result struct shape
(let [[result (ev/spawn (fn [] (subprocess/system "echo" ["test"])))]]
  (assert-true (integer? (get result :exit))   "subprocess/system: :exit is integer")
  (assert-true (string?  (get result :stdout)) "subprocess/system: :stdout is string")
  (assert-true (string?  (get result :stderr)) "subprocess/system: :stderr is string"))

# subprocess/system: concurrent subprocesses
(let [[f1 (ev/spawn (fn [] (get (subprocess/system "echo" ["one"]) :stdout)))]
      [f2 (ev/spawn (fn [] (get (subprocess/system "echo" ["two"]) :stdout)))]]
  (assert-eq f1 "one\n" "concurrent subprocess/system: fiber 1")
  (assert-eq f2 "two\n" "concurrent subprocess/system: fiber 2"))

# ── subprocess/exec: sequence args ───────────────────────────────────────────

# subprocess/exec accepts cons list args
(assert-eq
  (ev/spawn (fn []
    (let [[proc (subprocess/exec "echo" (list "hello"))]]
      (string (stream/read-all (get proc :stdout))))))
  "hello\n"
  "subprocess/exec: list args work")

# subprocess/exec accepts empty list (no args)
(assert-eq
  (ev/spawn (fn []
    (subprocess/wait (subprocess/exec "true" ()))))
  0
  "subprocess/exec: empty list args work")

# subprocess/exec accepts @array args
(assert-eq
  (ev/spawn (fn []
    (let [[proc (subprocess/exec "echo" @["world"])]]
      (string (stream/read-all (get proc :stdout))))))
  "world\n"
  "subprocess/exec: @array args work")

# subprocess/exec rejects non-sequence args with type-error
(assert-err-kind
  (fn [] (ev/spawn (fn [] (subprocess/exec "echo" "not-a-sequence"))))
  :type-error
  "subprocess/exec: string args gives type-error")

# subprocess/exec rejects non-string element in list with type-error
(assert-err-kind
  (fn [] (ev/spawn (fn [] (subprocess/exec "echo" (cons 42 ())))))
  :type-error
  "subprocess/exec: non-string element in list gives type-error")

# ── subprocess/system: sequence args (pass-through via subprocess/exec) ───────
#
# subprocess/system passes args straight through to subprocess/exec, so
# sequence widening is free. These tests confirm the pass-through works end
# to end without any dedicated subprocess/system logic.

# subprocess/system accepts a list for args
(assert-eq
  (ev/spawn (fn [] (get (subprocess/system "echo" (list "hi")) :stdout)))
  "hi\n"
  "subprocess/system: list args work")

# subprocess/system accepts empty list
(assert-eq
  (ev/spawn (fn [] (get (subprocess/system "true" ()) :exit)))
  0
  "subprocess/system: empty list args work")

# subprocess/system accepts @array for args
(assert-eq
  (ev/spawn (fn [] (get (subprocess/system "echo" @["bye"]) :stdout)))
  "bye\n"
  "subprocess/system: @array args work")

# ── sys/env ──────────────────────────────────────────────────────────────────

# sys/env returns a struct
(assert-true (struct? (sys/env)) "sys/env: returns a struct")

# sys/env contains PATH (always set on Linux)
(assert-true (string? (get (sys/env) "PATH")) "sys/env: PATH is a string")

# sys/env arity is enforced by the PrimitiveDef layer (Arity::Exact(0)),
# not inside the function body — consistent with sys/args.

# ── sys/args ─────────────────────────────────────────────────────────────────
#
# sys/args integration tests require subprocess invocation (spawning elle with
# -- separator), which cannot be done from within Elle. Those tests are in the
# Rust test suite: tests/integration/sys_args.rs
