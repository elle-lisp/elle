# process

<!-- audited: 2026-09-23 -->

The submodules behind [process.lisp](../process.lisp): the scheduler in four parts, the primitives, and one file per behavior.

[process.md](../process.md) is the caller's guide. This file holds what no
single submodule owns: how the scheduler divides, and the rules the parts
share.

| File | Purpose |
|------|---------|
| [primitives.lisp](primitives.lisp) | The yielding primitives: `send`, `recv`, `spawn`, `link`, `monitor`, … |
| [core.lisp](core.lisp) | The process table: mailboxes, links, monitors, exits, timers |
| [waits.lisp](waits.lisp) | Sub-fibers, and the join, select, abort and futex waits |
| [commands.lisp](commands.lisp) | What each yielded primitive does to the table |
| [scheduler.lisp](scheduler.lisp) | The round loop, forwarded I/O, orphan teardown, `start` and `run` |
| [genserver.lisp](genserver.lisp) | GenServer, and Actor on top of it |
| [task.lisp](task.lisp) | Task |
| [supervisor.lisp](supervisor.lisp) | Supervisor, and the subprocess child spec |
| [event.lisp](event.lisp) | EventManager, a GenServer with handler modules |

## How the scheduler divides

`make-scheduler` builds the parts in dependency order. Each part is a module
closure that takes the parts before it and returns a struct:

```text
core  →  waits (core)  →  commands (core)  →  scheduler (core, waits, commands)
```

Core owns every container the parts share: the process table, the ready and
waiting queues, the timers, the pending forwarded I/O and the futex parks.
Nothing rebinds a container after core builds it. A part that must replace a
queue's contents does so in place, through `refill`, so every part keeps
holding the same object.

## Waiters

A wait op comes from a process fiber or from a sub-fiber that a process
spawned. Both are *waiters*: a process is its pid, and a sub-fiber is the
`sub-waiter` struct `{:fiber :pid}`. One `wake` in waits.lisp resumes either kind, so
join, select, abort, park and notify each have one implementation for both.

A process waiter is queued and runs in the next round. A sub-fiber waiter runs
at once, inside the wake, and is routed by `after-resume` like any sub-fiber
that has just run.

## Behaviors

[process.lisp](../process.lisp) imports the primitives once and passes them to
each behavior module. A behavior never imports another module itself,
because a `.lisp` import compiles the file again on every call.
