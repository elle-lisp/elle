# Mediation

<!-- audited: 2026-10-11 -->

What a parent does with a denial it catches: perform the call for the child, refuse it, or leave it to the scheduler that refuses it.

[capabilities.md](capabilities.md) owns the withheld set, the denial and its
payload. This document starts where the parent holds a denial.

## Granting

The parent can catch a denial, perform the operation on the child's
behalf, and resume the child with the result:

```lisp
(let [f (fiber/new
           (fn [] (length "hello"))
           |:error|
           :deny |:error|)]
  (let [denial (fiber/resume f)]
    # denial is {:error :capability-denied :primitive "length" ...}
    (let [val (fiber/value f)]
      (let [result (apply length (val :args))]
        (fiber/resume f result)))))
```

## Refusing

Resuming with an ordinary value tells the child the call succeeded and
returned that value. A mediator that wants to say *no* must not do that:
agent-written code reads the resume value as `file/write`'s return and
proceeds as though the write happened.

`fiber/refuse` raises the denied call as a failure **at the child's own
call site**. The child's `protect` or `try` catches it, and the child
keeps running:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "notes")
        f (fiber/new
             (fn []
               (let [[ok? err] (protect (file/write target "x"))]
                 (list :refused (not ok?) err)))
             |:fs :error|
             :deny |:fs|)]
    (fiber/resume f)
    (fiber/refuse f :not-permitted)
    (fiber/value f)))
# => (:refused true :not-permitted)
```

The fiber stays alive. A refused call is an ordinary event in a mediated
session — the child is told no and carries on, and may be refused again
on its next call.

To refuse a call, deny the bit that names its authority, such as `:fs`, not
`:error`. The child's own `protect` runs primitives that declare `:error`, so
a fiber denied that bit has no working recovery path for the refusal to land
in.

An uncaught refusal stops the child `:error` at the refused call. With
`:error` in the child's mask, `fiber/refuse` answers the refusal and
`fiber/value` holds it. Without `:error`, the refusal raises past
`fiber/refuse` into the mediator, as an error passes `fiber/resume`:

```lisp
(let [f (fiber/new (fn [] (file/read "/etc/hostname")) |:fs| :deny |:fs|)]
  (fiber/resume f)
  (let [[ok? err] (protect (fiber/refuse f :not-permitted))]
    (assert (not ok?))
    (assert (= err :not-permitted))
    (assert (= (fiber/status f) :error))))
```

The mediator then decides what the stopped child does. `fiber/resume`
restarts it at the refused call, and the resume value answers the call
exactly as a grant would. `fiber/cancel` ends it `:dead`, and the code past
the call never runs:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "notes")
        f (fiber/new (fn [] (list :wrote (file/write target "x")))
                     |:fs :error| :deny |:fs|)]
    (fiber/resume f)
    (assert (= (fiber/refuse f :not-permitted) :not-permitted))
    (assert (= (fiber/status f) :error))
    (assert (= (fiber/value f) :not-permitted))
    (assert (= (fiber/resume f 1) (list :wrote 1)))   # the restart answers the call
    (assert (not (path/exists? target)))))            # and nothing was written

(let [ran @[]
      f (fiber/new (fn [] (file/read "/etc/hostname") (push ran :continued))
                   |:fs :error| :deny |:fs|)]
  (fiber/resume f)
  (fiber/refuse f :not-permitted)
  (fiber/cancel f)
  (assert (= (fiber/status f) :dead))
  (assert (empty? ran)))                              # the continuation never ran
```

To end a fiber outright rather than refuse one call, use `fiber/cancel`.

## A fiber a scheduler runs

`ev/spawn` creates a fiber in the calling fiber and hands it to the scheduler,
which resumes it. The spawned fiber therefore carries its creator's denial. Its
resumer is the scheduler, though, and the scheduler is not the mediator that
imposed the denial.

So the scheduler refuses the call. It hands the fiber `fiber/refuse`, with the
denial payload as the error, and the fiber's `protect` sees that payload at its
own call site. An uncaught refusal ends the fiber, and `ev/join` raises the
payload in the fiber that joins it:

```lisp
(with-temp-dir dir
  (let [target (path/join dir "x")
        sandbox (fiber/new
                  (fn []
                    (let [caught (ev/join (ev/spawn (fn [] (protect (file/write target "x")))))
                          raised (protect (ev/join (ev/spawn (fn [] (file/write target "x")))))]
                      [caught raised]))
                  |:fs :error| :deny |:fs|)
        [[ok1? denial] [ok2? raised]] (fiber/resume sandbox)]
    (assert (not ok1?) "the spawned fiber's protect sees the refusal")
    (assert (= (get denial :error) :capability-denied) "carrying the denial")
    (assert (= (get denial :primitive) "file/write") "which names the call")
    (assert (not ok2?) "an uncaught refusal ends the fiber, and the join raises it")
    (assert (= (get raised :error) :capability-denied))
    (assert (not (path/exists? target)) "nothing is written")))
```

A spawned fiber's mask names the bits a denial raises, `:ffi`, `:exec`, `:gpu`,
`:os-signal`, `:fs` and `:net`, as well as the bits the scheduler's own protocol
uses. A mediator above the scheduler therefore never sees a spawned fiber's
denial. The process scheduler does the same for a process and for each fiber a
process spawns, and `process:spawn` creates the new process's fiber in the fiber
that calls it ([process-scheduler.md](../process-scheduler.md)).
