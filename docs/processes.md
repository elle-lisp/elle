# Processes

`lib/process.lisp` provides Erlang-style concurrent processes built on
Elle's fiber scheduler. Processes have mailboxes, links, monitors, named
registration, and fuel-based preemption. On top of the core process API,
the module provides GenServer (callback-based servers), Actor (state
wrapper), Supervisor (automatic restart), and Task (one-shot async work).

## Loading

```text
(def process ((import "std/process")))
```

## Starting a process system

`process:start` creates a scheduler and runs a closure as the first
process. It blocks until all processes complete and returns the scheduler.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (println "hello from process 0")))
```

Use `process:run` when you need a pre-configured or shared scheduler:

```text
(def sched (process:make-scheduler :fuel 500))
(process:run sched (fn [] (println "on existing scheduler")))
```

## Sending and receiving messages

Every process has a mailbox. `send` delivers a message; `recv` blocks
until one arrives.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([me (process:self)])
    (process:send me :hello)
    (assert (= (process:recv) :hello) "got :hello"))))
```

## Spawning processes

`spawn` creates a new process. `spawn-link` links the child to the
parent (crash propagation). `spawn-monitor` monitors without linking
(death notification without crashing the parent).

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let* ([me (process:self)]
         [peer (process:spawn (fn []
                 (match (process:recv)
                   ([from :ping] (process:send from :pong))
                   (_ nil))))])
    (process:send peer [me :ping])
    (assert (= (process:recv) :pong) "ping-pong works"))))
```

## Selective receive

`recv-match` takes a predicate and returns the first message that
matches, leaving non-matching messages in the mailbox in order.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([me (process:self)])
    (process:send me :a)
    (process:send me :b)
    (process:send me :c)
    # Pick :b out of order
    (assert (= (process:recv-match (fn [m] (= m :b))) :b) "got :b")
    # Remaining arrive in original order
    (assert (= (process:recv) :a) "got :a")
    (assert (= (process:recv) :c) "got :c"))))
```

`recv-timeout` returns `:timeout` if no message arrives within the
given number of scheduler ticks.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (assert (= (process:recv-timeout 1) :timeout) "timed out")))
```

## Links and crash propagation

Linked processes crash together. When a linked child crashes, the parent
crashes too — unless the parent is trapping exits.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (process:trap-exit true)
  (let ([child (process:spawn-link (fn []
                 (error {:error :boom :message "crash"})))])
    (match (process:recv)
      ([:EXIT pid reason]
        (assert (= pid child) "EXIT from child")
        (match reason
          ([:error _] (assert true "got error reason"))
          (_ nil)))
      (_ nil)))))
```

## Monitors

Monitors deliver a `[:DOWN ref pid reason]` message when the monitored
process dies, without affecting the monitoring process.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([[child-pid ref] (process:spawn-monitor (fn [] :done))])
    (match (process:recv)
      ([:DOWN got-ref got-pid reason]
        (assert (= got-ref ref) "correct ref")
        (match reason
          ([:normal val] (assert (= val :done) "normal exit"))
          (_ nil)))
      (_ nil)))))
```

## Named processes

Processes can register under a keyword name. `whereis` looks up PIDs
by name; `send-named` sends to a registered name.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([me (process:self)])
    (process:spawn (fn []
      (process:register :greeter)
      (let ([msg (process:recv)])
        (match msg
          ([from name] (process:send from (string "hello, " name)))
          (_ nil)))))
    # sync to let the child register
    (process:send me :sync)
    (process:recv)
    (process:send-named :greeter [me "elle"])
    (assert (= (process:recv) "hello, elle") "named send works"))))
```

## Process dictionary

Each process has a private key-value store. Useful for per-process
configuration that doesn't belong in the main state.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (process:put-dict :counter 0)
  (process:put-dict :counter 42)
  (assert (= (process:get-dict :counter) 42) "dict works")
  (process:erase-dict :counter)
  (assert (nil? (process:get-dict :counter)) "erased")))
```

## Fuel-based preemption

Processes are cooperatively scheduled with fuel budgets. A CPU-bound
process gets preempted after exhausting its fuel, allowing other
processes to run.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([me (process:self)])
    # Busy-looper gets preempted
    (let ([busy (process:spawn (fn []
            (letrec ([loop (fn [n] (loop (+ n 1)))]) (loop 0))))])
      (process:spawn (fn [] (process:send me :done)))
      (assert (= (process:recv) :done) "worker runs despite busy-looper")
      (process:exit busy :kill))))
  :fuel 100)
```


# GenServer

GenServer is a callback-based generic server. You provide an `init`
function and handlers for calls (synchronous), casts (asynchronous),
and info messages (raw mailbox messages).

## Callbacks

```text
{:init        (fn [arg] state)
 :handle-call (fn [request from state] [:reply reply new-state])
 :handle-cast (fn [request state]      [:noreply new-state])
 :handle-info (fn [msg state]          [:noreply new-state])
 :terminate   (fn [reason state]       ...)}
```

`handle-call` can also return `[:noreply state]` for deferred replies
(use `gen-server-reply` later) or `[:stop reason reply state]` to shut
down after replying.

## Key-value store example

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([pid (process:gen-server-start-link
               {:init        (fn [_] @{})
                :handle-call (fn [request _from state]
                  (match request
                    ([:get key]    [:reply (get state key nil) state])
                    ([:put key val] (put state key val)
                                    [:reply :ok state])
                    (_ [:reply :unknown state])))}
               nil :name :kv)])
    (process:gen-server-call :kv [:put :lang "elle"])
    (assert (= "elle" (process:gen-server-call :kv [:get :lang]))
            "kv store works"))))
```

## Stopping a server

`gen-server-stop` requests graceful shutdown. The server's `:terminate`
callback runs before it exits.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([me (process:self)])
    (process:gen-server-start-link
      {:init        (fn [_] :running)
       :handle-call (fn [req _from state] [:reply state state])
       :terminate   (fn [reason state]
         (process:send me [:terminated reason]))}
      nil :name :stoppable)
    (process:gen-server-stop :stoppable :reason :shutdown)
    (match (process:recv)
      ([:terminated reason]
        (assert (= reason :shutdown) "clean shutdown"))
      (_ nil)))))
```

## Deferred replies

Sometimes the server can't reply immediately. Return `[:noreply state]`
from `handle-call` and use `gen-server-reply` later:

```text
{:handle-call (fn [request from state]
  # Stash `from` — reply later from handle-info
  [:noreply {:pending from}])
 :handle-info (fn [msg state]
  (process:gen-server-reply (get state :pending) msg)
  [:noreply nil])}
```


# Actor

Actor wraps GenServer with a simpler API: just an init function and
get/update operations on state.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (process:actor-start-link (fn [] 0) :name :counter)
  (process:actor-update :counter (fn [n] (+ n 1)))
  (process:actor-update :counter (fn [n] (+ n 1)))
  (process:actor-update :counter (fn [n] (+ n 1)))
  (assert (= 3 (process:actor-get :counter (fn [n] n)))
          "counter is 3")))
```


# Task

Task runs a one-shot function as a supervised process and returns the
result. Like `ev/spawn` but the work has a PID and can be monitored.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let* ([t1 (process:task-async (fn [] (* 6 7)))]
         [t2 (process:task-async (fn [] (+ 10 20)))]
         [r1 (process:task-await t1)]
         [r2 (process:task-await t2)])
    (assert (= r1 42) "task 1")
    (assert (= r2 30) "task 2"))))
```


# Supervisor

Supervisors manage child processes and restart them according to a
policy when they crash.

## Child specs

Each child is a struct with:

```text
{:id      :worker-name        # unique identifier
 :start   (fn [] ...)         # closure to run as a process
 :restart :permanent}         # :permanent | :transient | :temporary
```

- **`:permanent`** — always restart (even on normal exit)
- **`:transient`** — restart only on abnormal exit (crash)
- **`:temporary`** — never restart

## Strategies

| Strategy | Behavior |
|----------|----------|
| `:one-for-one` | Restart only the crashed child |
| `:one-for-all` | Restart all children when one crashes |
| `:rest-for-one` | Restart crashed child and all children started after it |

## Basic supervisor

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let ([me (process:self)])
    (process:supervisor-start-link
      [{:id :worker :restart :permanent
        :start (fn []
          (process:send me [:started (process:self)])
          (forever
            (match (process:recv)
              (:crash (error {:error :boom :message "crash"}))
              (:ping  (process:send me :pong))
              (_ nil))))}]
      :name :sup)

    # Wait for initial start
    (match (process:recv)
      ([:started pid1]
        (process:send pid1 :ping)
        (assert (= :pong (process:recv)) "child responds")
        # Crash it
        (process:send pid1 :crash)
        # Supervisor restarts it
        (match (process:recv)
          ([:started pid2]
            (assert (not (= pid1 pid2)) "new pid")
            (process:send pid2 :ping)
            (assert (= :pong (process:recv)) "restarted child responds"))
          (_ nil)))
      (_ nil)))))
```

## Restart intensity limits

Without limits, a child that crashes immediately on startup causes an
infinite restart loop. The `:max-restarts` and `:max-ticks` options
set a sliding window: if a child restarts more than N times within M
scheduler ticks, the supervisor stops restarting it.

```text
(process:supervisor-start-link children
  :max-restarts 3    # at most 3 restarts...
  :max-ticks 5)      # ...within 5 scheduler ticks
```

## Supervisor logging

Pass a `:logger` callback to receive structured lifecycle events:

```text
(process:supervisor-start-link children
  :logger (fn [event]
    (println "supervisor:" (get event :event) (get event :id))))
```

Events emitted:

| Event | Fields |
|-------|--------|
| `:child-started` | `:id`, `:pid` |
| `:child-exited` | `:id`, `:pid`, `:reason` |
| `:child-restarting` | `:id`, `:attempt` |
| `:max-restarts-reached` | `:id`, `:shutting-down` |

## Startup ordering with readiness signals

By default, children start concurrently. When a child spec includes
`:ready true`, the supervisor waits for that child to call
`supervisor-notify-ready` before starting the next child. This ensures
startup ordering — for example, a ZMQ bridge must bind its endpoints
before clients connect.

```text
(process:supervisor-start-link
  [{:id :bridge :restart :permanent :ready true
    :start (fn []
      (bind-zmq-endpoints)
      (process:supervisor-notify-ready)  # supervisor proceeds
      (forever (process:recv)))}
   {:id :client :restart :permanent
    :start (fn []
      # bridge is guaranteed ready at this point
      (connect-to-bridge)
      (forever (process:recv)))}])
```

If a child crashes before signaling readiness, the supervisor detects
the death and proceeds without deadlocking.

## Dynamic children

Add and remove children at runtime:

```text
(process:supervisor-start-child :sup
  {:id :dynamic-1 :restart :temporary
   :start (fn [] (forever (process:recv))})

(process:supervisor-stop-child :sup :dynamic-1)
(process:supervisor-which-children :sup)  # => [{:id ... :pid ...} ...]
```


# Supervised subprocesses

`make-subprocess-child` creates a child spec that manages an OS
subprocess under a supervisor. The child process spawns the subprocess,
blocks on `subprocess/wait`, then crashes on non-zero exit to trigger
supervisor restart.

```text
(process:supervisor-start-link
  [(process:make-subprocess-child :nginx "/usr/sbin/nginx" ["-g" "daemon off;"])
   (process:make-subprocess-child :redis "/usr/bin/redis-server" ["--port" "6380"]
     :restart :transient)]
  :name :daemon-sup
  :max-restarts 5
  :max-ticks 10
  :logger (fn [event] (println "daemon-sup:" event)))
```

This replaces the manual bridge pattern:

```text
# Before: every user writes this glue
{:id :my-daemon :restart :permanent
 :start (fn []
   (let ([proc (subprocess/exec "/usr/bin/my-daemon" [])])
     (let ([code (subprocess/wait proc)])
       (error {:error :subprocess-exit :code code})))}

# After: one-liner
(process:make-subprocess-child :my-daemon "/usr/bin/my-daemon" [])
```

Options passed to `subprocess/exec` (environment, working directory)
go in the `:opts` named argument:

```text
(process:make-subprocess-child :worker "/usr/bin/worker" []
  :opts {:cwd "/var/lib/worker" :env {:PORT "8080"}})
```


# EventManager

EventManager provides pub/sub event dispatching. Handlers are modules
with `:init`, `:handle-event`, and optional `:terminate` callbacks.

```text
(def handler-mod
  {:init         (fn [_] @[])
   :handle-event (fn [event state]
     (push state event)
     [:ok state])
   :terminate    (fn [reason state] nil)})

(process:event-manager-start-link :name :events)
(def ref (process:event-manager-add-handler :events handler-mod nil))
(process:event-manager-sync-notify :events :something-happened)
(process:event-manager-remove-handler :events ref)
```


# Structured concurrency inside processes

`ev/spawn` and `ev/join` work inside processes. Sub-fibers are
tracked by the scheduler and participate in I/O completion.

```elle
(def process ((import "std/process")))

(process:start (fn []
  (let* ([f1 (ev/spawn (fn [] (+ 10 20)))]
         [f2 (ev/spawn (fn [] (+ 30 40)))]
         [r1 (ev/join f1)]
         [r2 (ev/join f2)])
    (assert (= r1 30) "f1 = 30")
    (assert (= r2 70) "f2 = 70"))))
```


# Process API reference

## Core

| Function | Description |
|----------|-------------|
| `start init` | Create scheduler, run init as first process |
| `run sched init` | Run init on existing scheduler |
| `make-scheduler` | Create scheduler (`:fuel`, `:backend`) |
| `self` | Current process PID |
| `spawn fn` | Start new process |
| `spawn-link fn` | Start linked (crash propagation) |
| `spawn-monitor fn` | Start monitored (death notification) |
| `send pid msg` | Send message |
| `recv` | Block until message arrives |
| `recv-match pred` | Receive first matching message |
| `recv-timeout ticks` | Receive with timeout |

## Links and monitors

| Function | Description |
|----------|-------------|
| `link pid` | Link to another process |
| `unlink pid` | Remove link |
| `monitor pid` | Monitor another process |
| `demonitor ref` | Remove monitor |
| `trap-exit flag` | Catch linked exits as messages |
| `exit pid reason` | Terminate a process |

## Registration

| Function | Description |
|----------|-------------|
| `register name` | Register current process under keyword |
| `unregister name` | Remove registration |
| `whereis name` | Look up PID by name |
| `send-named name msg` | Send to registered name |

## Timers

| Function | Description |
|----------|-------------|
| `send-after ticks pid msg` | Delayed message delivery |
| `cancel-timer ref` | Cancel a pending timer |

## Process dictionary

| Function | Description |
|----------|-------------|
| `put-dict key val` | Store value, returns old |
| `get-dict key` | Retrieve value |
| `erase-dict key` | Remove key, returns old |

## GenServer

| Function | Description |
|----------|-------------|
| `gen-server-start-link callbacks init-arg` | Start linked server |
| `gen-server-call server request` | Synchronous call (`:timeout`) |
| `gen-server-cast server request` | Asynchronous cast |
| `gen-server-stop server` | Graceful shutdown (`:reason`, `:timeout`) |
| `gen-server-reply from reply` | Deferred reply |

## Actor

| Function | Description |
|----------|-------------|
| `actor-start-link init-fn` | Start linked actor (`:name`) |
| `actor-get actor fn` | Read derived state |
| `actor-update actor fn` | Transform state (sync) |
| `actor-cast actor fn` | Transform state (async) |

## Task

| Function | Description |
|----------|-------------|
| `task-async fn` | Spawn linked task, returns `[pid ref]` |
| `task-await task` | Wait for result (`:timeout`) |

## Supervisor

| Function | Description |
|----------|-------------|
| `supervisor-start-link children` | Start supervisor (`:name`, `:strategy`, `:max-restarts`, `:max-ticks`, `:logger`) |
| `supervisor-start-child sup spec` | Add child at runtime |
| `supervisor-stop-child sup id` | Remove and stop child |
| `supervisor-which-children sup` | List active children |
| `supervisor-notify-ready` | Signal readiness (child calls this) |
| `make-subprocess-child id bin args` | Create child spec for OS subprocess (`:opts`, `:restart`) |

## EventManager

| Function | Description |
|----------|-------------|
| `event-manager-start-link` | Start event manager (`:name`) |
| `event-manager-add-handler mgr mod arg` | Add handler, returns ref |
| `event-manager-remove-handler mgr ref` | Remove handler |
| `event-manager-notify mgr event` | Async broadcast |
| `event-manager-sync-notify mgr event` | Sync broadcast |
| `event-manager-which-handlers mgr` | List handlers |

## External API

| Function | Description |
|----------|-------------|
| `process-info sched pid` | Query process state from outside |
| `inject sched pid msg` | Send message from outside scheduler |


---

## See also

- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
- [fibers](signals/fibers.md) — fiber architecture underlying processes
- [runtime.md](runtime.md) — fuel budgets
- [io.md](io.md) — ports, subprocesses
- [scheduler.md](scheduler.md) — async event loop
