# Process behaviors

<!-- audited: 2026-09-16 -->

The callback-driven roles `lib/process.lisp` builds on the bare process:
GenServer, Actor, Task, Supervisor and EventManager.

Each one is a process. [processes.md](processes.md) owns the model underneath
them — mailboxes, links, monitors, registration and preemption — and this
document owns what each role adds.

```lisp
(def process ((import "std/process")))
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

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [pid (process:gen-server-start-link
               {:init        (fn [_] @{})
                :handle-call (fn [request _from state]
                  (match request
                    [:get key]     [:reply (get state key nil) state]
                    [:put key val] (begin (put state key val)
                                    [:reply :ok state])
                    _              [:reply :unknown state]))}
               nil :name :kv)]
    (process:gen-server-call :kv [:put :lang "elle"])
    (assert (= "elle" (process:gen-server-call :kv [:get :lang]))
            "kv store works"))))
```

## Stopping a server

`gen-server-stop` requests graceful shutdown. The server's `:terminate`
callback runs before it exits.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:gen-server-start-link
      {:init        (fn [_] :running)
       :handle-call (fn [req _from state] [:reply state state])
       :terminate   (fn [reason state]
         (process:send me [:terminated reason]))}
      nil :name :stoppable)
    (process:gen-server-stop :stoppable :reason :shutdown)
    (match (process:recv)
      [:terminated reason]
        (assert (= reason :shutdown) "clean shutdown")
      _ nil))))
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

```lisp
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

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let* [t1 (process:task-async (fn [] (* 6 7)))
         t2 (process:task-async (fn [] (+ 10 20)))
         r1 (process:task-await t1)
         r2 (process:task-await t2)]
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

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let [me (process:self)]
    (process:supervisor-start-link
      [{:id :worker :restart :permanent
        :start (fn []
          (process:send me [:started (process:self)])
          (forever
            (match (process:recv)
              :crash (error {:error :boom :message "crash"})
              :ping  (process:send me :pong)
              _ nil)))}]
      :name :sup)

    # Wait for initial start
    (match (process:recv)
      [:started pid1]
        (begin
          (process:send pid1 :ping)
          (assert (= :pong (process:recv)) "child responds")
          # Crash it
          (process:send pid1 :crash)
          # Supervisor restarts it
          (match (process:recv)
            [:started pid2]
              (begin
                (assert (not (= pid1 pid2)) "new pid")
                (process:send pid2 :ping)
                (assert (= :pong (process:recv)) "restarted child responds"))
            _ nil))
      _ nil))))
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
   (let [proc (subprocess/exec "/usr/bin/my-daemon" [])]
     (let [code (subprocess/wait proc)]
       (error {:error :subprocess-exit :code code}))))}

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


## API reference

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


---

## See also

- [processes.md](processes.md) — the process model these roles are built on
- [subprocess.md](subprocess.md) — the subprocess `make-subprocess-child` spawns
- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
