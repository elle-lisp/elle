# Bodies across epochs 17 and 18

<!-- audited: 2026-11-20 -->

The epoch 17 and 18 rules: a `def` outside a body moves into a `let`, and an early-read `def` moves up or joins a `letrec`.

[epochs.md](../epochs.md) states how migration runs and lists every rule. The
two rules here read each body of the tree, so they run after every other rule,
epoch 17's first. [bindings](../bindings.md) states the scope each epoch gives a
`def`.

## DefIntoLet

Moves a `def`, `var` or `defn` that stands outside a body into a `let`
([bindings](../bindings.md)). The rule carries no data, and runs after every rule
in the range but `DefsBeforeReads`.

Migration runs before expansion, so the rule reads only the forms whose shape
it knows without expanding them:

| Where the `def` stands | What takes its place |
|------------------------|----------------------|
| a form after the first operand of `when`, `unless`, `while` or `repeat`, or any form of `forever` | a `let` from the `def` to the end of the form |
| any other operand of a form the rule knows: a branch of `if`, a clause of `cond`, an operand of `and` or `or`, a test, a binding's value | `(let [x v] x)` |
| a `begin` or `do` anywhere but directly in a body the rule knows | a `let` in its place |

Each `def` in a run of them becomes one binding of the same `let`. A `var`
binds its name with `@`, and a `defn` binds its name to a `fn`. The forms after
the run are the body of the `let`, so a `def` among them already stands in a
body. When no form follows the run, the body reads the last name, so the form
keeps its value. The top of a quasiquote template may land in a body, so a
`begin` there stays.

The rule leaves a `def` as written where a `let` could change what a read
finds. That is a read of its name in the body around it, outside the new `let`
and before that body binds the name again. A read before the `let` counts,
because a loop or an earlier function can reach the `def`. So does a read in
the `def`'s own value or in an earlier binding of the same `let`.

A read does not count when another `def` of the name stands before it in one
sequence with it: a body, a `begin`, or the forms of a `when`, `unless`,
`while`, `repeat` or `forever` after the test. Up to epoch 16 that `def` binds
the name again, so the read finds that `def`, never the one that moves. Two
forms that each define and then read one name therefore move apart:

```text
(g (begin (def a 1) a))        (g (let [a 1] a))
(g (begin (def a 1) a))        (g (let [a 1] a))
```

A read inside a form that binds the name again finds that binding, so it does
not count either. Such a form is a `fn`, a `let` form, an `each`, or one of the
prelude's `if-let`, `when-let`, `when-ok`, `with` and `with-temp-dir`. The
`else` of an `if-let` stands inside the binding of its first name.

The rule also leaves three kinds of `def` it cannot read. One stands directly
in a form the rule does not know, such as a call or a user's macro. One has a
name that a template fills in. One binds a pattern, and no form follows it to
give the value. The compiler then reports each `def` the rule leaves, and `elle
rewrite` names it.

## DefsBeforeReads

Carries a body across epoch 18, which binds a name from its `def` onward
([bindings](../bindings.md)). The rule carries no data, and runs after
`DefIntoLet`. It reads the forms of each body that `DefIntoLet` knows.

A forward read is a read of a name inside a lambda, in a form that stands
before the body's first `def` of that name. The rule first moves each `def` a
forward read reaches above the read, where the move cannot change what a form
does. It then binds each forward read that is left in a `letrec`.

A `def` moves only when it runs no code: its value is a function (a `defn`,
or a `fn` form), a literal, a quoted form, or a name. It moves only above
forms that run no code either: such defs, and `defmacro` forms. So no code
that runs can tell that it moved. Up to epoch 17 a call that reached a
function before its `def` failed, and a move would make that call succeed.
The move must also leave every read finding what it found:

- The `def` binds one name, without `@`, and it is the body's only `def` of
  that name.
- It stands directly in the body, not in a `begin`, and it is not the body's
  last form, whose value is the body's value.
- No form it moves above reads its name directly. Up to epoch 17 such a read
  found a binding outside the body.
- Each `def` its value reads that would then stand after it moves too, and a
  forward read in a function it holds counts. A `def` that cannot move keeps
  every `def` that reads it in place.

Defs that read each other move together. The later ones also move up to the
first of them, so the `letrec` they need holds them alone. A moved `def` lands
directly above the first form that reads it, after each moved `def` it reads,
and moved defs otherwise keep the order they were written in. The comment
lines directly above a `def` move with it. Each moved `def` counts as one
rewrite.

A forward read the moves leave, such as one between functions that call each
other or one past a form that runs code, becomes a `letrec`. The rule turns
the run of defs from the reading `def` through the `def` it reads into one
`letrec`, and joins runs that overlap. A `var` binds its name with `@`, and a
`defn` binds its name to a `fn`. The forms after the run are the body of the
`letrec`, and when no form follows, the body reads the last name.

Up to epoch 17, a direct read of a name in its own `def`'s value reads the
binding before that `def`. The rule turns such a `def` into a `let` over the
forms after it, whose value reads that binding too.

The rule leaves a run as written when a `letrec` cannot hold it or would change
what a read finds. That is a run that holds a form other than a `def`, binds a
name twice, or holds a direct read of a name that the same or a later `def` in
it binds. It also leaves a forward read in a form other than a `def`, and a
direct read of a `def`'s own name where a lambda in its value reads the name
too. At epoch 18 a forward read the rule leaves may find an outer binding
instead, so the compiler refuses the file and names the read. `elle rewrite`
names each form the rule leaves.

