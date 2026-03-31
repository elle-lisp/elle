# I/O

All I/O in Elle is async — reads and writes yield to the scheduler. User
code runs inside the async scheduler automatically.

## Ports

Ports are bidirectional file descriptors. Open with `port/open`, close
with `port/close`.

```text
(def p (port/open "data.txt" :read))
(defer (port/close p)
  (println (port/read-all p)))
```

### Port operations

```text
(port/open path mode)        # mode: :read, :write, :append, :read-write
(port/read p n)              # read n bytes
(port/read-line p)           # read until \n, nil on EOF
(port/read-all p)            # read everything
(port/write p data)          # write bytes or string
(port/flush p)               # flush buffers
(port/seek p offset)         # seek to byte offset (default: from start)
(port/seek p off :from :end) # seek from end
(port/tell p)                # current byte position
(port/close p)               # close port
```

### Streams from ports

```text
(port/lines p)               # lazy stream of lines
(port/chunks p n)            # lazy stream of byte chunks
(port/writer p)              # writable stream
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

```text
# Run to completion — returns {:exit :stdout :stderr}
(subprocess/system "echo" ["hello"])
# => {:exit 0 :stdout "hello\n" :stderr ""}

# With options
(subprocess/system "ls" ["-la"] {:cwd "/tmp"})
(subprocess/system "env" [] {:env {:FOO "bar"}})

# Low-level control
(def proc (subprocess/exec "cat" []))
(subprocess/pid proc)
(subprocess/wait proc)
(subprocess/kill proc)
```

## System args and environment

```text
# Command-line args after the source file
# elle script.lisp -- foo bar  =>  ("--" "foo" "bar")
(def args (sys/args))
(def real-args (drop 1 args))  # skip "--"

# Environment
(sys/env)              # => struct of all env vars
(sys/env "HOME")       # => single var, or nil
```

---

## See also

- [concurrency.md](concurrency.md) — ev/spawn, ev/join, parallel I/O
- [fibers.md](fibers.md) — fiber-based async model
- [strings.md](strings.md) — string operations
