(elle/epoch 12)
# audited: 2026-09-17
# subprocess/exec through wait, kill, pid and exit — the type, its reads, and
# what every primitive refuses.


# ── subprocess/exec ──────────────────────────────────────────────────────────────

# subprocess/exec: what a subprocess reads as
(let [proc (subprocess/exec "echo" ["hello"])]
  (assert (subprocess? proc) "subprocess/exec: answers a subprocess")
  (assert (integer? (get proc :pid)) "subprocess/exec: :pid is integer")
  (assert (port? (get proc :stdout)) "subprocess/exec: :stdout is port")
  (assert (port? (get proc :stderr)) "subprocess/exec: :stderr is port")
  (assert (port? (get proc :stdin)) "subprocess/exec: :stdin is port")
  (assert (> (get proc :pid) 0) "subprocess/exec: pid > 0")
  (subprocess/wait proc))

# subprocess/exec: the handle is not a key
#
# The counter-factual is the struct this replaced, where `:process` held the
# external and a caller could pass it to `subprocess/wait` on its own. Reading
# nil here is what says there is nothing left to pass separately.
(let [proc (subprocess/exec "true" [])]
  (assert (nil? (get proc :process)) "subprocess/exec: :process is not a key")
  (assert (not (has? proc :process)) "subprocess/exec: and has? agrees")
  (subprocess/wait proc))

# subprocess?: false for everything that is not one
(assert (not (subprocess? {:pid 1 :stdin nil}))
        "subprocess?: a struct is not one")
(assert (not (subprocess? 42)) "subprocess?: an integer is not one")
(assert (not (subprocess? nil)) "subprocess?: nil is not one")

# A subprocess names itself, and prints the pid a reader needs.
#
# The trap: a keyword carries a name hash, not a spelling, and an external type
# name reaches a reader only if it is in the static vocabulary
# (src/value/keyword.rs). A name that is missing there prints as its hash, so
# this asserts the spelling rather than merely that type-of answered something.
(let [proc (subprocess/exec "true" [])]
  (assert (= (type-of proc) :subprocess) "type-of: names the type")
  (assert (= (string (type-of proc)) "subprocess")
          "type-of: and the name has a spelling to print")
  (assert (= (string proc) (string "#<subprocess " (get proc :pid) ">"))
          "a subprocess prints as its type and its pid")
  (subprocess/wait proc))

# subprocess/exec: stdout is binary by default (bytes, not string)
(let [raw (let [proc (subprocess/exec "echo" ["hello"])]
            (port/read-all (get proc :stdout)))]
  (assert (bytes? raw) "subprocess/exec: stdout is bytes"))

# subprocess/exec: decode bytes to string
(assert (= (let [proc (subprocess/exec "echo" ["hello"])]
             (string (port/read-all (get proc :stdout)))) "hello\n")
        "subprocess/exec: stdout bytes decode to string")

# subprocess/exec: binary output (head -c 4 /dev/urandom)
(let [raw (let [proc (subprocess/exec "head" ["-c" "4" "/dev/urandom"])]
            (port/read-all (get proc :stdout)))]
  (assert (bytes? raw) "subprocess/exec: binary output is bytes")
  (assert (= (length raw) 4) "subprocess/exec: binary output is 4 bytes"))

# subprocess/exec: stdin :null — no stdin pipe
(let [proc (subprocess/exec "echo" ["hi"] {:stdin :null})]
  (assert (nil? (get proc :stdin)) "subprocess/exec :stdin :null: stdin is nil")
  (subprocess/wait proc))

# ── subprocess/wait ──────────────────────────────────────────────────────────────

# subprocess/wait: exit 0
(assert (= (subprocess/wait (subprocess/exec "true" [])) 0)
        "subprocess/wait: /bin/true exits 0")

# subprocess/wait: exit 1
(assert (= (subprocess/wait (subprocess/exec "false" [])) 1)
        "subprocess/wait: /bin/false exits 1")

# subprocess/wait, kill and pid each refuse a value that is not a subprocess.
#
# The trap: this is the whole point of the type. The struct these replaced was
# accepted by all three whenever it carried a `:process` key, whatever sat
# under it, and each primitive then failed some steps later.
#
# The counter-factual is a struct shaped like the old exec result: it has the
# key, so the extractor that read the key without checking it let this through.
#
# That all three refuse it with ONE message body is what says the check is at
# the boundary, and it is asserted where a message may be read:
# `every_subprocess_primitive_refuses_a_non_subprocess_alike`
# (src/primitives/subprocess/tests.rs). A corpus file matches no message text
# (tests/elle/AGENTS.md).
(let [decoy {:pid 1 :stdin nil :stdout nil :stderr nil :process 42}]
  (each [name thunk] [["subprocess/wait" (fn [] (subprocess/wait decoy))]
                      ["subprocess/kill" (fn [] (subprocess/kill decoy))]
                      ["subprocess/pid" (fn [] (subprocess/pid decoy))]]
    (let [[ok? err] (protect (thunk))]
      (assert (not ok?) (string name ": a struct is refused"))
      (assert (= (get err :error) :type-error)
              (string name ": refused as a type-error")))))

# ── subprocess/pid ───────────────────────────────────────────────────────────────

# subprocess/pid: returns positive integer matching :pid field
(let [proc (subprocess/exec "sleep" ["10"])]
  (assert (> (subprocess/pid proc) 0) "subprocess/pid: returns positive integer")
  (assert (= (subprocess/pid proc) (get proc :pid))
          "subprocess/pid: matches :pid field")
  (subprocess/kill proc 15)
  (subprocess/wait proc))

# subprocess/pid: a reaped child still reports the number it had
#
# The counter-factual: answering nil or raising once the status is in would
# split one fact into two answers, since the exec result's :pid field keeps
# handing out the same number regardless.
(let [proc (subprocess/exec "true" [])]
  (subprocess/wait proc)
  (assert (= (subprocess/pid proc) (get proc :pid))
          "subprocess/pid: still matches :pid after the child is reaped"))

# ── reading a subprocess ─────────────────────────────────────────────────────

# get, has?, keys and values agree on one closed key set, in a fixed order.
#
# The trap: a struct's keys come back in TableKey hash order, so a caller who
# learned the order from the struct this replaced learned nothing portable. The
# key set is declared by the type, so its order is ours and is asserted here.
(let [proc (subprocess/exec "cat" [])]
  (assert (= (keys proc) '(:pid :stdin :stdout :stderr :exit))
          "keys: the closed set, in declaration order")
  (assert (= (length (values proc)) 5) "values: one per key")
  (assert (has? proc :stdout) "has?: a key it has")
  (assert (not (has? proc :nope)) "has?: a key it does not")
  (assert (nil? (get proc :nope)) "get: an unknown key reads nil")
  (assert (= (get proc :nope :fallback) :fallback)
          "get: and takes the default it was given")
  # Accessor syntax is its own form in the reader, desugared to a `get` call
  # rather than written as one, so it reaches this key set only if the
  # desugaring does. `demos/h2cross.lisp` reads a subprocess this way.
  (assert (= proc:pid (get proc :pid)) "accessor syntax reads the same key set")
  (assert (port? proc:stdout) "and reaches the ports through it")
  (port/close (get proc :stdin))
  (subprocess/wait proc))

# A port read out of a subprocess still reads after the binding it came from
# has ended.
#
# The trap: a subprocess holds its ports as heap values inside an external, and
# nothing enumerates them — not the alloc-time scan, not the free-time cascade
# (src/io/request/process.rs). What carries this port past the inner binding is
# the read itself: `get` hands back a value from the subprocess's region and
# increfs that region, and the binding here owns that reference until it ends.
# Run it under `elle --trace=guardfree`, where a read of freed memory traps
# instead of answering plausible bytes.
(let [out (let [proc (subprocess/exec "echo" ["outlives"])]
            (subprocess/wait proc)
            (get proc :stdout))]
  (assert (= (string (port/read-all out)) "outlives\n")
          "a port outlives the binding its subprocess was reached through"))

# A subprocess is read-only: the writes refuse it.
#
# The counter-factual is a struct, which takes all three and hands back a copy
# carrying a pid that names nothing.
(let [proc (subprocess/exec "true" [])]
  (each [name thunk] [["put" (fn [] (put proc :pid 1))]
                      ["del" (fn [] (del proc :pid))]
                      ["merge" (fn [] (merge proc {:pid 1}))]]
    (let [[ok? err] (protect (thunk))]
      (assert (not ok?) (string name ": refuses a subprocess"))
      (assert (= (get err :error) :type-error)
              (string name ": refuses it as a type-error"))))
  (subprocess/wait proc))

# ── subprocess/exit ──────────────────────────────────────────────────────────

# :exit reads the recorded status without waiting for it.
#
# The trap: reading it must not reap. A read that called waitpid would take the
# status from the kernel, and the subprocess/wait below would then have nothing
# to answer from — so asserting the wait's own answer is what catches it.
(let [proc (subprocess/exec "false" [])]
  (assert (nil? (get proc :exit)) ":exit is nil while nothing has reaped it")
  (assert (nil? (subprocess/exit proc)) "subprocess/exit agrees")
  (assert (= (subprocess/wait proc) 1) "the wait still answers /bin/false's 1")
  (assert (= (get proc :exit) 1) ":exit answers the same status afterwards")
  (assert (= (subprocess/exit proc) 1) "and so does subprocess/exit"))

# A signalled child's status is the negated signal, the same number the wait
# answers.
(let [proc (subprocess/exec "sleep" ["60"])]
  (subprocess/kill proc :sigkill)
  (assert (= (subprocess/wait proc) -9) "a SIGKILLed child waits to -9")
  (assert (= (get proc :exit) -9) "and :exit reads the same number"))

# ── subprocess/kill ──────────────────────────────────────────────────────────────

# subprocess/kill: send SIGTERM, wait, exit is nonzero
(let [exit (let [proc (subprocess/exec "sleep" ["60"])]
             (subprocess/kill proc 15)
             (subprocess/wait proc))]
  (assert (not (= exit 0)) "subprocess/kill: killed process has nonzero exit"))

# subprocess/kill: with explicit signal number 9 (SIGKILL)
(let [exit (let [proc (subprocess/exec "sleep" ["60"])]
             (subprocess/kill proc 9)
             (subprocess/wait proc))]
  (assert (not (= exit 0)) "subprocess/kill SIGKILL: nonzero exit"))

# subprocess/kill: keyword :sigterm terminates the process
(let [exit (let [proc (subprocess/exec "sleep" ["60"])]
             (subprocess/kill proc :sigterm)
             (subprocess/wait proc))]
  (assert (not (= exit 0)) "subprocess/kill :sigterm: nonzero exit"))

# subprocess/kill: the answer says whether a signal was sent
#
# The trap: a pid names a child only until somebody reaps it, and the kernel
# then hands the number out again. The second kill below must therefore make no
# syscall — the status the wait left on the handle is what says there is no
# child of ours left to signal.
#
# The counter-factual is :missing, which the ESRCH path answers. On this quiet
# machine the reaped pid names nobody, so a kill that DID reach the kernel would
# report :missing rather than raising; reading :exited is what says the record
# answered and the syscall never happened. Which process a kill would have
# reached needs a pid naming somebody else, and that is
# `a_kill_on_a_reaped_child_sends_no_signal` (src/primitives/subprocess/tests.rs).
(let [proc (subprocess/exec "sleep" ["60"])]
  (assert (= (subprocess/kill proc :sigterm) :signaled)
          "subprocess/kill: a live child is signaled")
  (subprocess/wait proc)
  (assert (= (subprocess/kill proc :sigterm) :exited)
          "subprocess/kill: a reaped child answers :exited"))

# ── child signal mask is reset ───────────────────────────────────────────────
#
# A spawned child must NOT inherit elle's internal signal mask. Elle blocks the
# absorb set on the main thread and ALL signals on worker threads (for its
# signalfd machinery); fork copies that mask and exec preserves it. If SIGTERM
# leaked in pending-blocked, the child would ignore `subprocess/kill … :sigterm`
# (only SIGKILL would land), wedging a clean shutdown.
#
# Assert the BEHAVIOR, not the mechanism (portable — no /proc, and it exercises
# the real worker-thread fork path under the runner): a child sent SIGTERM must
# die FROM SIGTERM (subprocess/wait → -15, the negated signal number), not
# survive to exit normally (→ 0). A leaked mask resolves in ~1s (the sleep runs
# out) rather than hanging, and reports the exit code it saw instead.
(let [proc (subprocess/exec "sleep" ["1"])]
  (subprocess/kill proc :sigterm)
  (let [exit (subprocess/wait proc)]
    (assert (= exit -15)
            (string "child did not die from SIGTERM (subprocess/wait=" exit
                    ") — elle's blocked signal mask leaked into the child"))))

# ── port/lines with subprocess ────────────────────────────────────────────────

# port/lines on subprocess stdout
(assert (= (let [proc (subprocess/exec "printf" ["a\\nb\\nc\\n"])]
             (stream/collect (port/lines (get proc :stdout))))
           (list "a" "b" "c")) "port/lines on subprocess stdout")

# ── stdin write ───────────────────────────────────────────────────────────────

# Write to subprocess stdin, read from stdout
(assert (= (let [proc (subprocess/exec "cat" [])]
             (port/write (get proc :stdin) "hello from stdin")
             (port/close (get proc :stdin))
             (string (port/read-all (get proc :stdout)))) "hello from stdin")
        "write stdin -> read stdout via cat")

# ── subprocess/system ────────────────────────────────────────────────────────────

# subprocess/system: basic success — exit code
(assert (= (get (subprocess/system "echo" ["hello"]) :exit) 0)
        "subprocess/system: echo exits 0")

# subprocess/system: stdout captured
(assert (= (get (subprocess/system "echo" ["hello"]) :stdout) "hello\n")
        "subprocess/system: echo stdout")

# subprocess/system: stderr captured and empty
(assert (= (get (subprocess/system "echo" ["hello"]) :stderr) "")
        "subprocess/system: echo stderr is empty")

# subprocess/system: nonzero exit
(let [result (subprocess/system "false" [])]
  (assert (not (= (get result :exit) 0))
          "subprocess/system: false has nonzero exit"))

# subprocess/system: result struct shape
(let [result (subprocess/system "echo" ["test"])]
  (assert (integer? (get result :exit)) "subprocess/system: :exit is integer")
  (assert (string? (get result :stdout)) "subprocess/system: :stdout is string")
  (assert (string? (get result :stderr)) "subprocess/system: :stderr is string"))

# subprocess/system: concurrent subprocesses
(let [results @[]]
  (let [f1 (ev/spawn (fn []
                       (push results
                             (get (subprocess/system "echo" ["one"]) :stdout))))
        f2 (ev/spawn (fn []
                       (push results
                             (get (subprocess/system "echo" ["two"]) :stdout))))]
    (ev/join f1)
    (ev/join f2))
  (assert (= (length results) 2) "concurrent subprocess/system: both complete")
  (assert (any? (fn [x] (= x "one\n")) results)
          "concurrent subprocess/system: one")
  (assert (any? (fn [x] (= x "two\n")) results)
          "concurrent subprocess/system: two"))

# ── subprocess/exec: sequence args ───────────────────────────────────────────

# subprocess/exec accepts pair list args
(assert (= (let [proc (subprocess/exec "echo" (list "hello"))]
             (string (port/read-all (get proc :stdout)))) "hello\n")
        "subprocess/exec: list args work")

# subprocess/exec accepts empty list (no args)
(assert (= (subprocess/wait (subprocess/exec "true" ())) 0)
        "subprocess/exec: empty list args work")

# subprocess/exec accepts @array args
(assert (= (let [proc (subprocess/exec "echo" @["world"])]
             (string (port/read-all (get proc :stdout)))) "world\n")
        "subprocess/exec: @array args work")

# subprocess/exec rejects non-sequence args with type-error
(let [[ok? err] (protect (subprocess/exec "echo" "not-a-sequence"))]
  (assert (not ok?) "subprocess/exec: string args gives type-error")
  (assert (= (get err :error) :type-error)
          "subprocess/exec: string args gives type-error"))

# subprocess/exec rejects non-string element in list with type-error
(let [[ok? err] (protect (subprocess/exec "echo" (pair 42 ())))]
  (assert (not ok?)
          "subprocess/exec: non-string element in list gives type-error")
  (assert (= (get err :error) :type-error)
          "subprocess/exec: non-string element in list gives type-error"))

# ── subprocess/system: sequence args (pass-through via subprocess/exec) ───────
#
# subprocess/system passes args straight through to subprocess/exec, so
# sequence widening is free. These tests confirm the pass-through works end
# to end without any dedicated subprocess/system logic.

# subprocess/system accepts a list for args
(assert (= (get (subprocess/system "echo" (list "hi")) :stdout) "hi\n")
        "subprocess/system: list args work")

# subprocess/system accepts empty list
(assert (= (get (subprocess/system "true" ()) :exit) 0)
        "subprocess/system: empty list args work")

# subprocess/system accepts @array for args
(assert (= (get (subprocess/system "echo" @["bye"]) :stdout) "bye\n")
        "subprocess/system: @array args work")

# ── sys/env ──────────────────────────────────────────────────────────────────

# sys/env returns a struct
(assert (struct? (sys/env)) "sys/env: returns a struct")

# sys/env contains PATH (always set on Linux)
(assert (string? (get (sys/env) "PATH")) "sys/env: PATH is a string")
# sys/env arity is enforced by the PrimitiveDef layer (Arity::Exact(0)),
# not inside the function body — consistent with sys/args.
# ── sys/args ─────────────────────────────────────────────────────────────────
#
# sys/args integration tests require subprocess invocation (spawning elle with
# -- separator), which cannot be done from within Elle. Those tests are in the
# Rust test suite: tests/integration/sys_args.rs
