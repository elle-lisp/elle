# process

<!-- audited: 2026-09-23 -->

Erlang-style processes on fibers: message passing, links, monitors, timers, named registration, and OTP-shaped behaviors above them.

Each submodule in [process/](process/overview.md) ends in the struct of what
it exports, [process.lisp](process.lisp) merges those structs, and
`(doc name)` carries a function's arguments. Four behaviors
are layered on the primitives — GenServer, Actor, Task and Supervisor —
plus an EventManager. This file holds the callback shapes, because a
caller writes those rather than calls them.

## Two worlds

The primitives — `send`, `recv`, `spawn`, `link`, `monitor`, `exit`,
`register` — yield, so they run inside a process and nowhere else.
Outside one, `make-scheduler`, `start`, `run`, `process-info` and
`inject` drive the scheduler from ordinary code. A process is preempted
on fuel, so no process can starve its siblings.

## Callback shapes

```lisp
# GenServer. `server` is a pid or a registered name; `from` is [pid ref].
{:init        (fn [arg] state)
 :handle-call (fn [request from state]
                [:reply reply state] | [:noreply state]
                | [:stop reason reply state])
 :handle-cast (fn [request state] [:noreply state] | [:stop reason state])
 :handle-info (fn [msg state]     [:noreply state] | [:stop reason state])
 :terminate   (fn [reason state] ...)}

# EventManager handler
{:init         (fn [arg] state)
 :handle-event (fn [event state] [:ok state] | [:remove state])
 :terminate    (fn [reason state] ...)}

# Supervisor child
{:id keyword
 :start (fn [] ...)
 :restart :permanent | :transient | :temporary}
```

A supervisor restarts under `:one-for-one` unless you name
`:one-for-all` or `:rest-for-one`.

## Running tests

```bash
elle tests/elle/process.lisp
elle tests/elle/genserver.lisp
elle tests/elle/supervisor.lisp
```
