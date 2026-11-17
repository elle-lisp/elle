# Migrations

<!-- audited: 2026-11-17 -->

Each epoch's breaking change, and the rewrite that carries an older file across it.

[Epochs](epochs.md) describes the mechanism: the declaration, the rule kinds,
and `elle rewrite`. This file lists what each epoch changed, oldest first.

## Epoch 1 — consolidate assertion helpers

Replaced `assert-true`, `assert-false`, `assert-eq`, `assert-equal`,
`assert-string-eq`, `assert-list-eq`, `assert-not-nil`, `assert-err`, and
`assert-err-kind` with the single `(assert expr msg)` form.

## Epoch 2 — print→println, newline→println, drop write

- `print` renamed to `println` (output with trailing newline).
- `newline` renamed to `println` (zero-arg newline is now `(println)`).
- `write` removed — use `(pp ...)` for literal form or `(port/write port data)` for port I/O.

## Epoch 3 — display→print

- `display` renamed to `print` (output without trailing newline).

After epoch 3, the output API is:

| Function   | Target   | Newline? |
|------------|----------|----------|
| `print`    | `*stdout*` | no     |
| `println`  | `*stdout*` | yes    |
| `eprint`   | `*stderr*` | no     |
| `eprintln` | `*stderr*` | yes    |

All four respect `parameterize` rebinding of `*stdout*`/`*stderr*`.

## Epoch 4 — stream/{read,read-line,read-all,write,flush} → port/...

Port I/O primitives moved from the `stream/` namespace to `port/`:

| Old name | New name |
|----------|----------|
| `stream/read-line` | `port/read-line` |
| `stream/read` | `port/read` |
| `stream/read-all` | `port/read-all` |
| `stream/write` | `port/write` |
| `stream/flush` | `port/flush` |

These five operations act exclusively on ports, not on abstract streams.
The `stream/` namespace now contains only stream combinators (`stream/map`,
`stream/filter`, `stream/collect`, etc.) which operate on lazy sequences.
`stream/read`, `stream/write` and `stream/flush` remain as aliases.

## Epoch 5 — polymorphic `has?`/`put`, retire string-specific containment

- `has?` is now the canonical membership predicate for structs, sets, and
  strings. `contains?` remains as a permanent alias.
- `string-contains?` renamed to `has?`.
- `string/contains?` renamed to `has?`.
- `put` now accepts 2 arguments for sets: `(put set value)`. The set-specific
  `add` is rewritten to `put` by `elle rewrite`.

## Epoch 6 — remove ev/run from user code

User code already runs in the async scheduler. `(ev/run (fn [] body...))`
is unwrapped to just `body...`. Non-lambda forms produce a compile error.

## Epoch 7 — flat let bindings

`let`, `let*`, `letrec`, `if-let`, `when-let`, and `when-ok` switch from
nested-pair bindings to flat (Clojure-style) bindings. Each binding is a
pattern/value pair laid out flat inside a single bracket form.

| Old (epoch ≤ 6) | New (epoch 7) |
|-----------------|---------------|
| `(let [[a 1] [b 2]] ...)` | `(let [a 1 b 2] ...)` |
| `(let [[[x y] (foo)]] ...)` | `(let [[x y] (foo)] ...)` |
| `(let* [[a 1] [b (+ a 1)]] ...)` | `(let* [a 1 b (+ a 1)] ...)` |
| `(if-let [[x (find-it)]] ...)` | `(if-let [x (find-it)] ...)` |

Destructuring is unambiguous: each form occupies exactly one syntactic
position, alternating pattern then value. `elle rewrite` handles the
migration mechanically.

## Epoch 8 — immutable-by-default bindings

All bindings (`def`, `let`, `let*`, `letrec`, function parameters) are
immutable by default. A mutable binding takes the `@` prefix, and a `var` from
epoch 7 or earlier becomes `def @`:

| Old (epoch ≤ 7) | New (epoch 8) |
|-----------------|---------------|
| `(var x 0)` | `(def @x 0)` |
| `(let [x 0] (assign x 1) ...)` | `(let [@x 0] (assign x 1) ...)` |
| `(defn f [n] (assign n 10))` | `(defn f [@n] (assign n 10))` |

The `@` prefix appears only at the binding site. All subsequent references
omit it. Assigning to a binding without `@` is a compile-time error:

```
cannot assign immutable binding 'x' (use @x to make it mutable)
```

Immutable bindings enable constant propagation by the compiler and
eliminate the need for cell indirection when captured by closures.

`elle rewrite` handles the `var → def @` migration. Mutable `let` and
parameter bindings require manual `@` annotation, guided by compile errors.

## Epoch 9 — flat cond/match clauses

`cond` and `match` clauses switch from parenthesized groups to flat pairs:

| Old (epoch ≤ 8) | New (epoch 9) |
|-----------------|---------------|
| `(cond (test1 body1) (test2 body2))` | `(cond test1 body1 test2 body2)` |
| `(match val (pat1 body1) (pat2 body2))` | `(match val pat1 body1 pat2 body2)` |

Multi-body arms are wrapped in `(begin ...)`. The `(else body)` form in
`cond` becomes a trailing default expression.

## Epoch 10 — cons→pair, car→first, cdr→rest

Classic Lisp pair operation names are replaced with descriptive alternatives:

| Old (epoch ≤ 9) | New (epoch 10) |
|-----------------|----------------|
| `cons` | `pair` |
| `car` | `first` |
| `cdr` | `rest` |

## Epoch 11 — sys/spawn→sys/spawn-vm, os/spawn→os/spawn-vm

`sys/spawn`/`os/spawn` now load the standard library in the spawned worker
(so `eval`/`read` resolve stdlib names there) and are correspondingly
heavier. The original light, primitives-only worker is now `sys/spawn-vm` /
`os/spawn-vm`. Renaming the old qualified names to their `-vm` form
preserves the original (light) behavior of pre-epoch code; new code gets the
heavy, stdlib-backed default via `sys/spawn`.

| Old (epoch ≤ 10) | New (epoch 11) |
|------------------|----------------|
| `sys/spawn` | `sys/spawn-vm` |
| `os/spawn` | `os/spawn-vm` |

Only the qualified `sys/spawn`/`os/spawn` are renamed. The bare `spawn`
symbol is deliberately left alone: `Rename` is not binding-aware, and
`spawn` is also the `(ev/scope (fn [spawn] …))` nursery parameter — a global
rename would clobber that local. Bare `spawn` is no longer registered as a
primitive alias at all, so top-level code must use `sys/spawn` /
`sys/spawn-vm`.

## Epoch 12 — remove coroutine API, use fibers directly

Coroutines were always trivial wrappers over yield-masking fibers and have
been removed. All `coro/*` and legacy `coroutine-*` / `make-coroutine`
forms are rewritten to their `fiber/*` equivalents.

| Old | New | Notes |
|-----|-----|-------|
| `coro/new fn` | `(fiber/new fn \|:yield\|)` | Adds yield capability |
| `make-coroutine fn` | `(fiber/new fn \|:yield\|)` | Gen-1 alias |
| `coro/resume` | `fiber/resume` | Rename |
| `coro/status` | `fiber/status` | Rename |
| `coro/done?` | `fiber/done?` | Rename |
| `coro/value` | `fiber/value` | Rename |
| `coroutine-resume` | `fiber/resume` | Gen-1 rename |
| `coroutine-status` | `fiber/status` | Gen-1 rename |
| `coroutine-done?` | `fiber/done?` | Gen-1 rename |
| `coroutine-value` | `fiber/value` | Gen-1 rename |
| `coroutine?` | `fiber?` | Rename |
| `coro?` | `fiber?` | Rename |
| `yield-from` | `yield*` | Rename |
| `coro/>iterator` | — | Removed (fibers are natively iterable) |
| `coroutine->iterator` | — | Removed |
| `coroutine-next` | — | Removed (use `fiber/resume`) |

## Epoch 13 — string escapes

A string literal gains three escapes, and an escape the reader does not know
becomes a read error. [syntax.md](syntax.md) lists the escapes epoch 13 reads.

| Escape | Epoch ≤ 12 | Epoch 13 |
|--------|------------|----------|
| `\0` | the string `0` | NUL, U+0000 |
| `\x41` | the string `x41` | `A`; `\x80` and above is an error |
| `\u{e9}` | the string `u{e9}` | `é` |
| `\q`, `\e`, `\'`, any other | the character after the backslash | read error |

Up to epoch 12 the reader knows five escapes: `\n`, `\t`, `\r`, `\\` and
`\"`. It drops the backslash of any other escape and reports nothing.

```lisp
# The same text, read under each epoch's rules.
(assert (= (read-all "\"\\x41\"") (list "A")) "epoch 13 reads the escape")
(assert (= (read-all "(elle/epoch 12) \"\\x41\"") (list '(elle/epoch 12) "x41"))
        "epoch 12 drops the backslash")
```

The change is lexical, so a file that declares epoch 12 or earlier keeps its
meaning. `elle rewrite` writes each escape whose meaning changed as the text
epoch 12 read from it, so `"\x41"` becomes `"x41"`. A file with no
declaration reads under the current epoch. When such a file holds an escape
that only an older epoch reads, the read error names the declaration that
restores the old reading.

## Epoch 14 — durations in seconds

Every call that waits takes its bound as `:timeout` in seconds, beside a new
`:deadline` ([I/O deadlines](io/timeout.md)). Up to epoch 13 the same calls
took milliseconds, some as a keyword and some as a positional argument.

| Epoch ≤ 13 | Epoch 14 |
|------------|----------|
| `(port/read p 64 :timeout 500)` | `(port/read p 64 :timeout 0.5)` |
| `(sys/join h 5000)` | `(sys/join h :timeout 5)` |
| `(chan/select rxs 50)` | `(chan/select rxs :timeout 0.05)` |
| `(io/wait b -1)` | `(io/wait b)` |
| `(ev/step)` | `(ev/step :timeout 0)` |

The keyword form covers every port call, the accepts and connects,
`udp/send-to`, `udp/recv-from` and `port/set-options`. The positional form
covers `sys/join`, `os/join`, `chan/select`, `chan/wait-ready`, `io/wait`,
`ev/step` and `ev/shutdown`. `(ev/step)` did not wait before, and a step with
no bound now does, so the old spelling gains `:timeout 0`. `elle rewrite`
applies the same rules, so a rewritten file reads its bounds in seconds.

## Epoch 15 — one form per signal concept

Each signal concept takes one form ([signals/inference.md](signals/inference.md)).
`squelch` takes its spec first, as `attune` does.

| Epoch ≤ 14 | Epoch 15 |
|------------|----------|
| `(silence)` | `(silent!)` |
| `(silence p)` | `(bound p \|\|)` |
| `(squelch f spec)` | `(squelch spec f)` |
| `(muffle spec)` | removed |

`muffle` declared, inside a function, a boundary that only its caller can
place. A file that declares epoch 14 or earlier and uses it does not compile,
and the error says to wrap the closure with `(squelch spec f)` instead.
`elle rewrite` applies the other three rules.

```lisp
(defn compiles? [src]
  (first (protect (compile/whole-module src "<doc>"))))

(assert (compiles? "(elle/epoch 14)\n(defn f [] (silence) 1)")
        "epoch 14 reads (silence) as (silent!)")
(assert (not (compiles? "(defn f [] (silence) 1)")) "epoch 15 has no silence")
(defn run [src]
  ((get (get (compile/whole-module src "<doc>") 0) 1)))

(def [ok? err] (run "(elle/epoch 14)\n(defn p [] (yield 1))\n(protect ((squelch p :yield)))"))
(assert (= :signal-violation (get err :error)) "epoch 14 reads the closure first")
(assert (not (compiles? "(elle/epoch 14)\n(defn f [] (muffle :yield) (yield 1))"))
        "muffle has no epoch 15 spelling")
```

## Epoch 16 — import-file goes

Up to epoch 15, `import-file` was a second name of `import`. Epoch 16 drops
the name: `import` emits a request that a loader answers, and a direct load
calls `import/load-file` or `import/load-plugin` ([modules.md](modules.md)).

| Epoch ≤ 15 | Epoch 16 |
|------------|----------|
| `(import-file path)` | `(import path)` |

The rule rewrites a call. `import` is a macro and has no value, so an
`import-file` passed as a value stays, and the compile names it undefined.
`elle rewrite` applies the same rule.

```lisp
(assert (compiles? "(elle/epoch 15)\n(defn f [] (import-file \"std/base64\"))")
        "epoch 15 reads import-file as import")
(assert (not (compiles? "(defn f [] (import-file \"std/base64\"))"))
        "epoch 16 has no import-file")
```

## Epoch 17 — a def binds only in a body

A `def` or `var` binds only where it stands directly in a body
([bindings](bindings.md)). Up to epoch 16 one could also stand in a branch, a
loop body or an operand, and a read after it found whichever `def` had run.

| Epoch ≤ 16 | Epoch 17 |
|------------|----------|
| `(when c (def x 1) (f x))` | `(when c (let [x 1] (f x)))` |
| `(while t (var i 0) (g i))` | `(while t (let [@i 0] (g i)))` |
| `(if c (def x 1) 2)` | `(if c (let [x 1] x) 2)` |
| `(f (begin (def x 1) x))` | `(f (let [x 1] x))` |

A `let` cannot keep the old reading where another read could find the `def`.
So the rule leaves the `def` as written when the body around it reads its name
outside the new `let`, or when its own value or an earlier `def` in the same
`let` reads it. It also leaves a `def` that a user's macro writes, because
migration runs before expansion ([epochs](epochs.md)). The compiler then
reports each such `def`, and `elle rewrite` names it.

```lisp
(def guarded "(defn f [c] (when c (def x 1) (+ x 1)))")
(assert (= (run (string "(elle/epoch 16)\n" guarded "\n(f true)")) 2)
        "epoch 16 reads the def as a let")
(assert (not (compiles? guarded)) "epoch 17 refuses the def")
(assert (not (compiles? "(elle/epoch 16)\n(defn f [c] (when c (def x 1)) x)"))
        "a def read after its form stays, and so does the error")
(assert (not (compiles? "(elle/epoch 16)\n(def y 0)
(defn f [] (var n 2) (while (> n 0) (println y) (def y n) (assign n (- n 1))))"))
        "a def the next pass of a loop reads stays too")
```
