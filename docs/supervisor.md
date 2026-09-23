# Supervisors

<!-- audited: 2026-09-23 -->

A supervisor starts child processes, restarts each one by its policy, and gives up when they crash too often.

A supervisor is a process. [processes.md](processes.md) owns the links and
exit signals it works through, and [behaviors.md](behaviors.md) owns the
GenServer, Actor and EventManager that it can run as children.

A supervisor traps exits and links to each child, so it learns of every
child's exit, even one that dies on its first instruction.

`supervisor-start-link` returns the supervisor's pid once the supervisor has
started every child in its list, and every `:ready` child has reported
ready or exited. A GenServer that a `:start-link` child starts has registered
its name by then, so the caller can call it at once.

## Child specs

Each child is a struct with:

```text
{:id         :worker-name     # unique identifier
 :start      (fn [] ...)      # the child's body, run as a new process
 :start-link (fn [] pid)      # or: spawns the child, returns its pid
 :restart    :permanent       # :permanent | :transient | :temporary
 :ready      false}           # wait for supervisor-notify-ready (:start only)
```

A spec names its child one of two ways:

- **`:start`** is the child's body. The supervisor spawns a process that
  runs it.
- **`:start-link`** is a function that spawns the child itself and returns
  its pid, as `gen-server-start-link`, `actor-start-link` and
  `event-manager-start-link` do. The supervisor calls it in its own process,
  links to the pid it returns, and supervises that process. A `:start-link`
  that raises crashes the supervisor.

The restart policy decides whether an exited child starts again:

- **`:permanent`** — always restart (even on normal exit)
- **`:transient`** — restart only on abnormal exit (crash)
- **`:temporary`** — never restart; the supervisor forgets the spec once the
  child exits

`supervisor-start-link` and `supervisor-start-child` raise `{:error
:invalid-child-spec}` for a spec with no `:id`, with both or neither of
`:start` and `:start-link`, or with `:ready` beside `:start-link`. They raise
it too for an `:id` that the supervisor already has.

## Strategies

| Strategy | Behavior |
|----------|----------|
| `:one-for-one` | Restart only the crashed child |
| `:one-for-all` | Restart all children when one crashes |
| `:rest-for-one` | Restart crashed child and all children started after it |

Each strategy restarts static children and children added with
`supervisor-start-child` alike.

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
            _ (assert false "expected [:started pid2]")))
      _ (assert false "expected [:started pid1]")))))
```

## Supervising a GenServer

A GenServer spawns its own process, so its spec uses `:start-link`. The
supervisor restarts the server itself, and the restarted server takes the
registered name again.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:supervisor-start-link
    [{:id :kv :restart :permanent
      :start-link (fn []
        (process:gen-server-start-link
          {:init        (fn [_] @{})
           :handle-call (fn [req _from state]
             (match req
               [:put k v] (begin (put state k v) [:reply :ok state])
               [:get k]   [:reply (get state k) state]))
           :handle-cast (fn [_req _state] (error {:error :boom :message "crash"}))}
          nil :name :kv))}]
    :name :sup)
  (process:gen-server-call :kv [:put :lang "elle"])
  (assert (= "elle" (process:gen-server-call :kv [:get :lang])) "the server answers")
  (let [first-pid (process:whereis :kv)]
    (process:gen-server-cast :kv :crash)
    (process:recv-timeout 20)
    (assert (not (= first-pid (process:whereis :kv))) "the supervisor restarted the server")
    (assert (nil? (process:gen-server-call :kv [:get :lang])) "with fresh state"))))
```

## Restart intensity limits

Without limits, a child that crashes immediately on startup causes an
infinite restart loop. The `:max-restarts` and `:max-ticks` options set a
sliding window over the scheduler clock (see [processes.md](processes.md)
for ticks). When one child is restarted more than `:max-restarts` times
within `:max-ticks` ticks, the supervisor logs `:max-restarts-reached`,
stops every child, and exits with the reason `:shutdown`.

`:max-ticks` defaults to 100. Without `:max-restarts`, restarts are
unbounded.

The supervisor's exit reaches its parent through their link. A parent that
traps exits receives `[:EXIT sup :shutdown]`; any other parent dies with it.

```lisp
(def process ((import "std/process")))

(process:start (fn []
  (process:trap-exit true)
  (let [sup (process:supervisor-start-link
              [{:id :flaky :restart :permanent
                :start (fn [] (error {:error :boom :message "boom"}))}]
              :max-restarts 3)]
    (assert (= (process:recv) [:EXIT sup :shutdown]) "the supervisor gave up"))))
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
| `:child-ready` | `:id`, `:pid` |
| `:child-exited` | `:id`, `:pid`, `:reason` |
| `:child-restarting` | `:id`, `:attempt` |
| `:max-restarts-reached` | `:id`, `:shutting-down` |

## Startup ordering with readiness signals

The supervisor starts its children in list order, and each child runs as soon
as it is spawned. When a child spec includes `:ready true`, the supervisor
waits for that child to call `supervisor-notify-ready` before it starts the
next child. For example, a ZMQ bridge must bind its endpoints before clients
connect.

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

A child that exits before it signals readiness does not block the supervisor.
The next child starts, and the exited child's restart policy applies to
it as to any other exit.

## Dynamic children

Add and remove children at runtime. A child that `supervisor-start-child`
adds belongs to the supervisor like a static one, and every strategy restarts
it. `supervisor-stop-child` stops a child and forgets its spec, so no strategy
starts it again.

`supervisor-start-child`, `supervisor-stop-child` and
`supervisor-which-children` call the supervisor as `gen-server-call` calls a
server. Each raises `{:error :gen-server-down}` when the supervisor exits
before it answers (see [behaviors.md](behaviors.md)).

```text
(process:supervisor-start-child :sup
  {:id :dynamic-1 :restart :temporary
   :start (fn [] (forever (process:recv))})

(process:supervisor-stop-child :sup :dynamic-1)
(process:supervisor-which-children :sup)  # => [{:id ... :pid ...} ...]
```

## Supervised subprocesses

`make-subprocess-child` creates a child spec that manages an OS
subprocess under a supervisor. The child process spawns the subprocess and
blocks on `subprocess/wait`. A non-zero exit code crashes the child, so the
supervisor restarts it.

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

The spec it builds is the one a caller would otherwise write by hand:

```text
{:id :my-daemon :restart :permanent
 :start (fn []
   (let [code (subprocess/wait (subprocess/exec "/usr/bin/my-daemon" [] {}))]
     (when (not (= code 0))
       (error {:error :subprocess-exit :code code}))))}
```

Options passed to `subprocess/exec` (environment, working directory)
go in the `:opts` named argument, which defaults to `{}`:

```text
(process:make-subprocess-child :worker "/usr/bin/worker" []
  :opts {:cwd "/var/lib/worker" :env {:PORT "8080"}})
```

## API reference

| Function | Description |
|----------|-------------|
| `supervisor-start-link children` | Start supervisor (`:name`, `:strategy`, `:max-restarts`, `:max-ticks`, `:logger`) |
| `supervisor-start-child sup spec` | Add child at runtime |
| `supervisor-stop-child sup id` | Stop child and forget its spec |
| `supervisor-which-children sup` | List active children |
| `supervisor-notify-ready` | Signal readiness (child calls this) |
| `make-subprocess-child id bin args` | Create child spec for OS subprocess (`:opts`, `:restart`) |

---

## See also

- [behaviors.md](behaviors.md) — GenServer, Actor, Task and EventManager
- [processes.md](processes.md) — the links and exit signals a supervisor works through
- [subprocess.md](subprocess.md) — the subprocess `make-subprocess-child` spawns
