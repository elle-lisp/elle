# process

<!-- audited: 2026-09-23 -->

Erlang-style processes on fibers: message passing, links, monitors, timers, named registration, and OTP-shaped behaviors above them.

Each submodule in [process/](process/overview.md) ends in the struct of what
it exports, [process.lisp](process.lisp) merges those structs, and
`(doc name)` carries a function's arguments. Four behaviors
are layered on the primitives — GenServer, Actor, Task and Supervisor —
plus an EventManager. This file holds the callback shapes, because a
caller writes those rather than calls them.
[docs/processes.md](../docs/processes.md), [docs/supervisor.md](../docs/supervisor.md) and
[docs/behaviors.md](../docs/behaviors.md) run each of them.

## Two worlds

The primitives — `send`, `recv`, `spawn`, `link`, `monitor`, `exit`,
`register` — yield, so they run inside a process and nowhere else.
Outside one, `make-scheduler`, `start`, `run`, `process-info` and
`inject` drive the scheduler from ordinary code. A process is preempted
on fuel, so no process can starve its siblings.

## Callback shapes

A GenServer is a struct of callbacks. `from` is `[pid ref]`, and a
server is named by its pid or by a registered keyword.

| Callback | Arguments | Returns |
|----------|-----------|---------|
| `:init` (required) | `arg` | the state, `[:ok state]`, or `[:stop reason]` |
| `:handle-call` | `request from state` | `[:reply reply state]`, `[:noreply state]`, or `[:stop reason reply state]` |
| `:handle-cast` | `request state` | `[:noreply state]` or `[:stop reason state]` |
| `:handle-info` (optional) | `msg state` | `[:noreply state]` or `[:stop reason state]` |
| `:terminate` (optional) | `reason state` | ignored |

An EventManager handler is a struct of callbacks too.

| Callback | Arguments | Returns |
|----------|-----------|---------|
| `:init` (required) | `arg` | the handler's state |
| `:handle-event` | `event state` | `[:ok state]`, or `[:remove state]` to leave |
| `:terminate` (optional) | `reason state` | ignored |

A supervisor child is a struct. It carries exactly one of `:start` and
`:start-link`.

| Key | Value |
|-----|-------|
| `:id` | a keyword naming the child |
| `:start` | a closure the supervisor runs as the child process |
| `:start-link` | a closure that spawns the child and returns its pid |
| `:restart` | `:permanent` (the default), `:transient` or `:temporary` |
| `:ready` | with `:start` only: `true` to hold the next child until this one calls `supervisor-notify-ready` |

A supervisor restarts under `:one-for-one` unless you name
`:one-for-all` or `:rest-for-one`. [supervisor.md](../docs/supervisor.md)
says what each field and strategy does, and [behaviors.md](../docs/behaviors.md)
says what each callback does.

## Running tests

```bash
elle tests/elle/process.lisp
elle tests/elle/genserver.lisp
elle tests/elle/supervisor.lisp
```
