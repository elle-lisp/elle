# I/O

All I/O in Elle is async — reads and writes yield to the scheduler. User
code runs inside the async scheduler automatically.

## I/O backend

On Linux, Elle uses `io_uring` for all I/O: file reads, writes, TCP,
timers, subprocess pipes. Operations are submitted to the kernel's
submission queue and completed without syscalls or threads — the kernel
handles multiplexing directly. A single-threaded event loop polls the
completion queue and resumes the waiting fiber.

On macOS, Elle uses a thread-pool backend that provides the same
abstraction. Blocking I/O operations run on background threads; the
event loop collects results and resumes fibers identically. User code
sees no difference — the same `port/open`, `port/read-line`, `ev/spawn`
API works on both platforms.

Both backends are syscall-free from the fiber's perspective: the fiber
yields `:io`, the scheduler submits the operation, and the fiber resumes
with the result. No threads are created per-operation on Linux; on macOS,
the thread pool is shared across all fibers.

## Ports

Ports are bidirectional file descriptors. Open with `port/open`, close
with `port/close`.

```lisp
(file/write "/tmp/elle-doc-test.txt" "hello from elle")
(def p (port/open "/tmp/elle-doc-test.txt" :read))
(defer (port/close p)
  (port/read-all p))          # => "hello from elle"
(file/delete "/tmp/elle-doc-test.txt")
```

### Port operations

```lisp
# (port/open path mode)        — mode: :read, :write, :append, :read-write
# (port/read p n)              — read n bytes
# (port/read-line p)           — read until \n, nil on EOF
# (port/read-all p)            — read everything
# (port/write p data)          — write bytes or string
# (port/flush p)               — flush buffers
# (port/seek p offset)         — seek to byte offset (default: from start)
# (port/tell p)                — current byte position
# (port/close p)               — close port
```

### Streams from ports

```lisp
# (port/lines p)               — lazy stream of lines
# (port/chunks p n)            — lazy stream of byte chunks
# (port/writer p)              — writable stream
```

## Output

```lisp
(print "no newline")
(println "with newline")
(println "count: " 42)         # multiple args concatenated
(eprint "to stderr")
(eprintln "error: bad input")
(pp {:a [1 2 3]})              # pretty-print data structures
```

All output functions are async — they yield to the scheduler.
`*stdout*` and `*stderr*` are dynamic parameters that can be rebound.

## Subprocesses

### Run to completion

`subprocess/system` runs a command and captures its output:

```lisp
# Run to completion — returns {:exit :stdout :stderr}
(subprocess/system "echo" ["hello"])
# => {:exit 0 :stdout "hello\n" :stderr ""}

# With options
(subprocess/system "ls" ["-la"] {:cwd "/tmp"})
(subprocess/system "env" [] {:env {:FOO "bar"}})
```

### Long-running subprocesses

`subprocess/exec` spawns a subprocess and returns a handle with stdio
ports. Use `subprocess/wait` to block until exit, `subprocess/kill` to
send signals.

```lisp
# Spawn and interact
(def proc (subprocess/exec "cat" []))
(port/write (get proc :stdin) "hello")
(port/close (get proc :stdin))
(string (port/read-all (get proc :stdout)))  # => "hello"
(subprocess/wait proc)                       # => 0

# Spawn, kill, reap
(def proc (subprocess/exec "sleep" ["60"]))
(subprocess/kill proc :sigterm)
(subprocess/wait proc)                       # => non-zero
```

### Subprocess options

```lisp
# (subprocess/exec program args)           — default: pipes for all stdio
# (subprocess/exec program args opts)      — with options struct
#
# Options:
#   :env    — struct of env vars (merged with inherited)
#   :cwd    — working directory string
#   :stdin  — :pipe (default) | :null | :inherit
#   :stdout — :pipe (default) | :null | :inherit
#   :stderr — :pipe (default) | :null | :inherit
```

### Supervised subprocesses

For long-running daemons, use `lib/process` to supervise OS subprocesses.
The supervisor automatically restarts them on crash:

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:supervisor-start-link
    [(process:make-subprocess-child :worker "/usr/bin/worker" []
       :opts {:env {:PORT "8080"}})
     (process:make-subprocess-child :monitor "/usr/bin/monitor" [])]
    :name :daemon-sup
    :max-restarts 5)))
```

See [processes.md](processes.md) for the full supervisor API.

## System args and environment

```lisp
# sys/args returns args after the source file
(def args (sys/args))

# Environment
(sys/env)              # => struct of all env vars
(sys/env "HOME")       # => single var, or nil
```

---

## See also

- [processes.md](processes.md) — supervised subprocesses, GenServer, actors
- [concurrency.md](concurrency.md) — ev/spawn, ev/join, parallel I/O
- [fibers](signals/fibers.md) — fiber-based async model
- [strings.md](strings.md) — string operations
