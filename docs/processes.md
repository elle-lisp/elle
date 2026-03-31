# Processes

`lib/process.lisp` provides Erlang-style concurrent processes built on
Elle's fiber scheduler.

## Loading

```text
(def process ((import-file "lib/process.lisp")))
```

## Core primitives

```text
(process:start thunk)          # run thunk in a process scheduler
(process:self)                 # current process ID
(process:spawn thunk)          # spawn a child process
(process:spawn-monitor thunk)  # spawn + monitor (returns [pid ref])

(process:send pid msg)         # send a message to a process
(process:recv)                 # receive the next message (blocks)
(process:recv-match pred)      # receive first matching message

(process:register name)        # register current process under a name
(process:send-named name msg)  # send to a named process
```

## Ping-pong example

```text
(process:start (fn []
  (let* ([me (process:self)]
         [peer (process:spawn (fn []
                 (match (process:recv)
                   ([from :ping] (process:send from :pong)))))])
    (process:send peer [me :ping])
    (process:recv))))    # => :pong
```

## Supervision

Monitors detect child crashes. `process:trap-exit` enables catching
exit signals as messages instead of propagating errors.

```text
(process:trap-exit true)
(def [pid ref] (process:spawn-monitor worker-fn))

# when child crashes, parent receives:
# [:DOWN ref pid {:error :crash ...}]
```

## GenServer pattern

```text
(process:spawn (fn []
  (process:register :kv-store)
  (var data @{})
  (forever
    (match (process:recv)
      ([from :get key]
        (process:send from (get data key)))
      ([from :put key val]
        (put data key val)
        (process:send from :ok))
      ([:stop] (break))))))
```

## Options

```text
(process:start thunk
  :backend backend         # share an io backend
  :fuel 1000)              # fuel budget per time slice
```

---

## See also

- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
- [fibers.md](fibers.md) — fiber architecture underlying processes
- [runtime.md](runtime.md) — fuel budgets
