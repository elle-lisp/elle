# Process behaviors

<!-- audited: 2026-09-23 -->

The callback-driven roles [lib/process.lisp](../lib/process.lisp) builds on the bare process:
GenServer, Actor, Task and EventManager.

Each one is a process. [processes.md](processes.md) owns the model underneath
them — mailboxes, links, monitors, registration and preemption — and this
document owns what each role adds. The supervisor that starts and restarts
them has its own document, [supervisor.md](supervisor.md).

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
down after replying. `handle-cast` and `handle-info` stop the server by
returning `[:stop reason state]`.

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
callback runs, and then the server exits with the reason that `:reason` names,
`:normal` by default.

`gen-server-start-link` links the server to the process that starts it, so the
server's exit reaches that process. A reason other than `:normal` kills it,
unless it traps exits.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:trap-exit true)
  (let [me (process:self)
        server (process:gen-server-start-link
                 {:init        (fn [_] :running)
                  :handle-call (fn [req _from state] [:reply state state])
                  :terminate   (fn [reason state]
                    (process:send me [:terminated reason]))}
                 nil :name :stoppable)]
    (process:gen-server-stop :stoppable :reason :shutdown)
    (assert (= (process:recv) [:terminated :shutdown]) ":terminate ran")
    (assert (= (process:recv) [:EXIT server :shutdown]) "the exit reached the starter"))))
```

## Timeouts

`gen-server-call` and `gen-server-stop` take `:timeout`, in scheduler ticks.
When no reply arrives in time, they raise `{:error :gen-server-timeout}`.
Without `:timeout`, they wait until the server replies or exits.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] nil)
     :handle-call (fn [_req _from state] [:noreply state])}
    nil :name :silent)
  (let [[ok? err] (protect (process:gen-server-call :silent :ping :timeout 5))]
    (assert (not ok?) "the call timed out")
    (assert (= (get err :error) :gen-server-timeout) "with :gen-server-timeout"))))
```

## A server that exits during a call

`gen-server-call` and `gen-server-stop` monitor the server while they wait.
When the server exits before it replies, they raise `{:error :gen-server-down
:reason r}`, where `r` is the server's exit reason. A call to a server that has
already exited raises at once, with the reason `:noproc`.

When the reply comes first, the call removes its monitor, and no `:DOWN` from
that monitor stays in the caller's mailbox. A link is separate from the call.
A caller that traps exits and is linked to the server still receives `[:EXIT
server reason]`.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:trap-exit true)
  (let* [server (process:gen-server-start-link
                  {:init        (fn [_] nil)
                   :handle-call (fn [_req _from _state]
                     (error {:error :boom :message "crash in call"}))}
                  nil :name :fragile)
         [ok? err] (protect (process:gen-server-call :fragile :ping))]
    (assert (not ok?) "the call raises")
    (assert (= (get err :error) :gen-server-down) "with :gen-server-down")
    (assert (= (first (get err :reason)) :error) "carrying the server's exit reason")
    (match (process:recv)
      [:EXIT pid [:error _]] (assert (= pid server) "the link delivers the exit as well")
      _ (assert false "expected [:EXIT server [:error ...]]")))))
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

Task runs a one-shot function as a process and returns its value to the caller.
It is like `ev/spawn`, except that the work has a PID.

`task-async` spawns the process and monitors it, and returns `[pid ref]`,
where `ref` is the monitor's ref. The task is not linked, so its crash does
not kill the caller. `task-await` returns the function's value. It
raises `{:error :task-error}` when the task crashed, and `{:error
:task-timeout}` when `:timeout` ticks pass first; a task that times out keeps
running. Once `task-await` returns or raises, no message from the task is left
in the caller's mailbox.

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

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (let* [t (process:task-async (fn [] (error {:error :boom :message "crash"})))
         [ok? err] (protect (process:task-await t))]
    (assert (not ok?) "awaiting a crashed task raises")
    (assert (= (get err :error) :task-error) "with :task-error"))))
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
| `task-async fn` | Spawn monitored task, returns `[pid ref]` |
| `task-await task` | Wait for result (`:timeout`) |

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
- [supervisor.md](supervisor.md) — the supervisor that starts and restarts them
- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
