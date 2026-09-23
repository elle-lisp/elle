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

## GenServer

GenServer is a callback-based generic server. You provide an `init`
function and handlers for calls (synchronous), casts (asynchronous),
and info messages (raw mailbox messages).

### Callbacks

The callbacks are a struct. `from` is `[pid ref]`.

| Callback | Arguments | Returns |
|----------|-----------|---------|
| `:init` (required) | `arg` | the state, `[:ok state]`, or `[:stop reason]` |
| `:handle-call` | `request from state` | `[:reply reply state]`, `[:noreply state]`, or `[:stop reason reply state]` |
| `:handle-cast` | `request state` | `[:noreply state]` or `[:stop reason state]` |
| `:handle-info` (optional) | `msg state` | `[:noreply state]` or `[:stop reason state]` |
| `:terminate` (optional) | `reason state` | ignored |

`handle-call` returns `[:noreply state]` to defer its reply (use
`gen-server-reply` later), and `[:stop reason reply state]` to shut down
after replying.

### Key-value store example

```lisp
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

### Stopping a server

`gen-server-stop` requests graceful shutdown. The server's `:terminate`
callback runs, and then the server exits with the reason that `:reason` names,
`:normal` by default.

`gen-server-start-link` links the server to the process that starts it, so the
server's exit reaches that process. A reason other than `:normal` kills it,
unless it traps exits.

```lisp
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

### Timeouts

`gen-server-call` and `gen-server-stop` take `:timeout`, in scheduler ticks.
When no reply arrives in time, they raise `{:error :gen-server-timeout}`.
Without `:timeout`, they wait until the server replies or exits.

```lisp
(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] nil)
     :handle-call (fn [_req _from state] [:noreply state])}
    nil :name :silent)
  (let [[ok? err] (protect (process:gen-server-call :silent :ping :timeout 5))]
    (assert (not ok?) "the call timed out")
    (assert (= (get err :error) :gen-server-timeout) "with :gen-server-timeout"))))
```

A call that has timed out leaves nothing in the caller's mailbox, even when the
server replies later. The call's ref works as an alias for the caller, and the
call turns the alias off when it ends. The scheduler drops a reply sent to an
alias that is off, and removes one that arrived before the call turned it off.

```lisp
(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] nil)
     :handle-call (fn [_req _from state]
       (process:recv-timeout 10)
       [:reply :late state])}
    nil :name :slow)
  (let [[ok? _] (protect (process:gen-server-call :slow :ping :timeout 2))]
    (assert (not ok?) "the call timed out"))
  (assert (= (process:recv-timeout 30) :timeout) "the late reply never arrives")))
```

### A server that exits during a call

`gen-server-call` and `gen-server-stop` monitor the server while they wait.
When the server exits before it replies, they raise `{:error :gen-server-down
:reason r}`, where `r` is the server's exit reason. A call to a server that has
already exited raises at once, with the reason `:noproc`.

When the reply comes first, the call removes its monitor, and no `:DOWN` from
that monitor stays in the caller's mailbox. A link is separate from the call.
A caller that traps exits and is linked to the server still receives `[:EXIT
server reason]`.

```lisp
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

### Deferred replies

Sometimes the server can't reply immediately. Return `[:noreply state]`
from `handle-call` and use `gen-server-reply` later. Here the call stashes
`from` and the reply leaves from `handle-info`:

```lisp
(process:start (fn []
  (process:gen-server-start-link
    {:init        (fn [_] nil)
     :handle-call (fn [request from _state]
                    # Finish the work later, when this message comes back.
                    (process:send (process:self) [:answer request])
                    [:noreply {:pending from}])
     :handle-info (fn [[_ request] state]
                    (process:gen-server-reply (get state :pending)
                                              [:answered request])
                    [:noreply nil])}
    nil :name :deferred)
  (assert (= [:answered :question] (process:gen-server-call :deferred :question))
          "the caller blocks until the deferred reply")))
```

A deferred reply goes through the same alias as any other. When the call has
already ended, because it timed out or raised, the reply goes nowhere.

## Actor

Actor wraps GenServer with a simpler API: just an init function and
get/update operations on state.

```lisp
(process:start (fn []
  (process:actor-start-link (fn [] 0) :name :counter)
  (process:actor-update :counter (fn [n] (+ n 1)))
  (process:actor-update :counter (fn [n] (+ n 1)))
  (process:actor-update :counter (fn [n] (+ n 1)))
  (assert (= 3 (process:actor-get :counter (fn [n] n)))
          "counter is 3")))
```

## Task

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
(process:start (fn []
  (let* [t1 (process:task-async (fn [] (* 6 7)))
         t2 (process:task-async (fn [] (+ 10 20)))
         r1 (process:task-await t1)
         r2 (process:task-await t2)]
    (assert (= r1 42) "task 1")
    (assert (= r2 30) "task 2"))))
```

```lisp
(process:start (fn []
  (let* [t (process:task-async (fn [] (error {:error :boom :message "crash"})))
         [ok? err] (protect (process:task-await t))]
    (assert (not ok?) "awaiting a crashed task raises")
    (assert (= (get err :error) :task-error) "with :task-error"))))
```

## EventManager

EventManager provides pub/sub event dispatching. Handlers are modules
with `:init`, `:handle-event`, and optional `:terminate` callbacks. A
handler that answers `[:remove state]` leaves, and its `:terminate` runs
with the reason `:remove`, as it does when the handler is removed.

```lisp
(process:start (fn []
  (let [me (process:self)
        recorder {:init         (fn [_] @[])
                  :handle-event (fn [event state]
                                  (push state event)
                                  [:ok state])
                  :terminate    (fn [reason state]
                                  (process:send me [reason (freeze state)]))}]
    (process:event-manager-start-link :name :events)
    (let [ref (process:event-manager-add-handler :events recorder nil)]
      (process:event-manager-sync-notify :events :something-happened)
      (process:event-manager-remove-handler :events ref)
      (assert (= [:remove [:something-happened]] (process:recv))
              "the handler saw the event and was told why it left")))))
```

## API reference

### GenServer

| Function | Description |
|----------|-------------|
| `gen-server-start-link callbacks init-arg` | Start linked server (`:name`) |
| `gen-server-call server request` | Synchronous call (`:timeout`) |
| `gen-server-cast server request` | Asynchronous cast; returns `:ok` |
| `gen-server-stop server` | Graceful shutdown (`:reason`, `:timeout`) |
| `gen-server-reply from reply` | Deferred reply |

### Actor

| Function | Description |
|----------|-------------|
| `actor-start-link init-fn` | Start linked actor (`:name`) |
| `actor-get actor fn` | Read derived state |
| `actor-update actor fn` | Transform state (sync) |
| `actor-cast actor fn` | Transform state (async) |

### Task

| Function | Description |
|----------|-------------|
| `task-async fn` | Spawn a monitored task, returns `[pid ref]` |
| `task-await task` | Wait for result (`:timeout`) |

### EventManager

| Function | Description |
|----------|-------------|
| `event-manager-start-link` | Start event manager (`:name`) |
| `event-manager-add-handler mgr mod arg` | Add handler, returns ref |
| `event-manager-remove-handler mgr ref` | Remove handler |
| `event-manager-notify mgr event` | Async broadcast |
| `event-manager-sync-notify mgr event` | Sync broadcast |
| `event-manager-which-handlers mgr` | List handlers as `{:id :mod}` structs |

## See also

- [processes.md](processes.md) — the process model these roles are built on
- [supervisor.md](supervisor.md) — the supervisor that starts and restarts them
- [concurrency.md](concurrency.md) — lower-level ev/spawn, ev/join
