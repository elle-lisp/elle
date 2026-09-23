# Subprocesses

<!-- audited: 2026-09-23 -->

Elle spawns a child process as a `subprocess` value: one thing to read streams
from, to signal, and to wait on.

## Run to completion

`subprocess/system` runs a command, captures its output, and answers a struct:

```lisp
(assert (= (get (subprocess/system "echo" ["hello"]) :stdout) "hello\n")
        "system captures the child's stdout as text")

(assert (= (get (subprocess/system "true" []) :exit) 0)
        "and reports the exit code beside it")
```

The answer is `{:exit int :stdout string :stderr string}` — a finished run's
captured bytes. It is a struct because that is all it is: the child is already
gone, so there is nothing left to wait on, signal, or read from. Use
`subprocess/exec` when you need the child while it runs.

`subprocess/system` takes the same options struct as `subprocess/exec`, except
that its `:stdin` defaults to `:null`.

```lisp
(assert (= (get (subprocess/system "pwd" [] {:cwd "/usr"}) :stdout) "/usr\n")
        ":cwd sets the child's working directory")

(assert (= (get (subprocess/system "sh" ["-c" "echo $FOO"] {:env {:FOO "bar"}})
                :stdout)
           "bar\n")
        ":env replaces the environment rather than adding to it")
```

## Spawning a child

`subprocess/exec` returns a `subprocess`. The child is running when it answers.

```lisp
(def cat (subprocess/exec "cat" []))
(port/write (get cat :stdin) "hello")
(port/close (get cat :stdin))
(assert (= (string (port/read-all (get cat :stdout))) "hello")
        "the child's stdout carries what its stdin was given")
(assert (= (subprocess/wait cat) 0) "and cat exits cleanly when its input ends")
```

`subprocess/wait` yields until the child exits and answers its exit code.
`subprocess/kill` signals it. Both take the subprocess itself.

```lisp
(def sleeper (subprocess/exec "sleep" ["60"]))
(assert (= (subprocess/kill sleeper :sigterm) :signaled)
        "the child is still there, so the signal reaches it")
(assert (not (= (subprocess/wait sleeper) 0))
        "a signalled child does not exit cleanly")
```

## Reading a subprocess

A subprocess answers `get` over a closed set of keys:

| Key | Answers |
|---|---|
| `:pid` | the OS process ID, as an integer |
| `:stdin` | a write port, or `nil` when stdin was not a pipe |
| `:stdout` | a read port, or `nil` when stdout was not a pipe |
| `:stderr` | a read port, or `nil` when stderr was not a pipe |
| `:exit` | the exit code once something has reaped the child, `nil` before |

`has?`, `keys` and `values` read the same set. `keys` answers in the order
above, which is fixed rather than sorted.

`has?` asks about the key, not the value, exactly as it does of a struct: every
key above is present on every subprocess, whatever it currently answers. Use
`nil?` on the value to ask whether a stream is there.

```lisp
(def lister (subprocess/exec "true" []))
(assert (= (keys lister) '(:pid :stdin :stdout :stderr :exit))
        "the key set is closed, and its order is part of the interface")
(assert (has? lister :stdout) "a key the type declares")
(assert (not (has? lister :process)) "and one it does not")
(assert (nil? (get lister :nope)) "an unknown key reads as nil")
(assert (= (get lister :nope :fallback) :fallback)
        "and takes the default get was given")
(subprocess/wait lister)
```

`:exit` reads the status the child left behind without waiting for it. It
answers `nil` while the child runs, and the same integer `subprocess/wait`
answers once the child is reaped.

```lisp
(def quick (subprocess/exec "false" []))
(assert (nil? (get quick :exit)) "nothing has reaped it yet")
(assert (= (subprocess/wait quick) 1) "/bin/false exits 1")
(assert (= (get quick :exit) 1) "and the status stays readable afterwards")
```

`subprocess/pid` and `subprocess/exit` answer `:pid` and `:exit` as named
primitives, for a call site that reads better without the keyword.

### A subprocess is read-only

`put`, `del` and `merge` refuse it. A subprocess reports what the child is
doing; it is not a place to write fields.

```lisp
(def target (subprocess/exec "true" []))
(let [[ok? err] (protect (put target :pid 1))]
  (assert (not ok?) "a subprocess refuses a write")
  (assert (= (get err :error) :type-error) "and refuses it as a type error"))
(subprocess/wait target)
```

## Why a subprocess is not a struct

A child process has identity. Two children that ran the same program are two
children, and the value that names one has to survive being compared, copied
and passed around without becoming the other.

A struct cannot carry that. Structs compare by their contents, so two structs
describing the same pid are equal, and a trait table attached to one does not
help — traits are invisible to equality, and `with-traits` lets any caller
attach one to anything (see [traits.md](traits.md)).

The `subprocess` type also settles what reaches `subprocess/wait`,
`subprocess/kill` and `subprocess/pid`. One shape arrives, and a value that is
not a subprocess is refused where it is passed rather than several steps later.

## Killing a child that may already be gone

A pid names a child only until somebody reaps it. The kernel then returns the
number to the pool and hands it out again, so a signal sent on a reaped child's
pid reaches whatever holds that number now — nothing on a quiet machine, another
child of this program or an unrelated process on a busy one.

So `subprocess/kill` asks the subprocess before it asks the kernel. When the
child's exit status is already recorded, the child is gone, and the call sends
no signal at all. The status gets there by `subprocess/wait` — or by any other
reap, since a reap is recorded rather than spent.

The answer says which happened, so a caller that cares can tell them apart.
Each one reports what the call observed, and nothing beyond it:

| Answer | What it means |
|---|---|
| `:signaled` | `kill(2)` took the signal for this child. |
| `:exited` | The exit status is recorded. No signal was sent. |
| `:missing` | No process holds that pid. Nothing was signalled. |

`:exited` and `:missing` are two different pieces of evidence, which is why they
are two answers. The recorded status says the child is gone and says whose child
it was; `ESRCH` says only that the number named nobody at that moment, and a pid
carries no record of who used to hold it.

All three are success. Killing a child that is already dead is not an error —
the state the kill asked for already holds — and a program that kills on a timer
while a fiber waits reaches this as a race rather than a mistake:

```lisp
(ev/run (fn []
          (let [child (subprocess/exec "sleep" ["30"])]
            (assert (= :signaled (subprocess/kill child :sigterm))
                    "the child is still there, so the signal goes to it")
            (subprocess/wait child)
            (assert (= :exited (subprocess/kill child :sigterm))
                    "the wait reaped it, so this kill has nothing to signal"))))
```

`:pid` still answers the number after a reap. The number is what the child had;
what a caller does with it afterwards is outside this runtime.

## A wait keeps the status it reaped

A wait that a deadline ends can still reap the child in the moment between the
cancel and the wait noticing, and a reap takes the status from the kernel for
good. So the status is kept on the subprocess rather than delivered and
forgotten: it goes to a waiter, or waits on the subprocess until one asks.

Every `subprocess/wait` on a child that has exited answers the same status,
however many times it is called and whatever became of the wait before it.

```lisp
(ev/run (fn []
          (let [child (subprocess/exec "sleep" ["30"])]
            (assert (nil? (ev/timeout 0.2 (fn [] (subprocess/wait child))))
                    "a child that outlives the deadline must not hold the wait")
            (subprocess/kill child :sigkill)
            (subprocess/wait child))))
```

`subprocess/wait` ends on cancellation but takes no `:timeout` of its own. See
[io.md](io.md) for the calls that wait, and what ends each one.

## Options

```lisp
# (subprocess/exec program args)           — default: pipes for all stdio
# (subprocess/exec program args opts)      — with options struct
#
# Options:
#   :env    — struct of env vars (replaces the environment)
#   :cwd    — working directory string
#   :stdin  — :pipe (default) | :null | :inherit
#   :stdout — :pipe (default) | :null | :inherit
#   :stderr — :pipe (default) | :null | :inherit
```

A stream that is not a pipe reads as `nil`, because there is no port to hand
back:

```lisp
(def quiet (subprocess/exec "echo" ["hi"] {:stdin :null}))
(assert (nil? (get quiet :stdin)) ":null leaves no port behind")
(assert (has? quiet :stdin) "the key is still there — has? asks about the key")
(subprocess/wait quiet)
```

Pipes are binary. Decoding subprocess output is the caller's work, through
`(string bytes-val)` or `port/lines`.

## Supervised subprocesses

For long-running daemons, use `lib/process` to supervise OS subprocesses. The
supervisor restarts them on crash:

```lisp
# (def process ((import "std/process")))
#
# (process:start (fn []
#   (process:supervisor-start-link
#     [(process:make-subprocess-child :worker "/usr/bin/worker" []
#        :opts {:env {:PORT "8080"}})
#      (process:make-subprocess-child :monitor "/usr/bin/monitor" [])]
#     :name :daemon-sup
#     :max-restarts 5)))
```

See [supervisor.md](supervisor.md) for the full supervisor API.

---

## See also

- [io.md](io.md) — ports, the async backend, and what bounds each call
- [supervisor.md](supervisor.md) — the supervisor that restarts a child on crash
- [processes.md](processes.md) — Erlang-style processes
- [posix-signals.md](posix-signals.md) — sending and observing POSIX signals
- [traits.md](traits.md) — trait tables, and why they are not identity
