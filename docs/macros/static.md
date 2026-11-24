# Macros: static definitions

<!-- audited: 2026-11-23 -->

A static definition computes a sealed value while the unit compiles, and binds it for macro bodies and for run-time code alike.

[overview.md](overview.md) defines the terms this document uses, and
[binding.md](binding.md) states which bindings are static without one.

## The form

`(static name expr)` evaluates `expr` when the analyzer reaches the form, and
binds `name` to the value from that form onward, where a `def` would bind. The
binding is immutable and static. The form's name is a working name
([plan.md](plan.md)).

The evaluation is an expansion in all but its result. It runs under the fuel
budget and without the capabilities an expansion withholds
([macros.md](../macros.md)), and a raise in it fails the compile.

A macro body reads the name as the value. Run-time code reads the name as a
constant of the unit, so one definition serves a macro that checks a field
name and a function that prints it:

```lisp
# (static point {:fields '[x y]})
#
# (defmacro new (record & args)
#   (let [info (meta/static-value record)]   # {:fields [x y]}
#     ...))
#
# (new point 1 2)                    # {:x 1 :y 2}
# (length point:fields)              # 2, read from the same constant
# (let [point (lookup)]
#   (new point 1 2))                 # a compile error: point is not static here
```

`(defmacro name params body)` is the static definition whose value is a macro.
`begin-for-syntax` goes, and an epoch rule rewrites each of its definitions to
a static definition ([epochs.md](../epochs.md)).

## What may be static

A static definition meets two tests.

Every free name of `expr` resolves to a static binding. The expression runs
before any run-time code of the unit, so no other binding holds a value yet.

The value is sealed. A string, an immutable collection, a syntax object, a
native function and a macro are sealed, and so is a closure that captures
sealed values through bindings no code assigns. A mutable collection, a box, a
fiber, a port and a handle are not, and the definition fails the compile naming
the part that is not.

Three facts need the second test:

- The value leaves the VM that compiled it. A file compiles on one VM and runs
  on another ([pipeline.md](../pipeline.md)), and an image carries the unit's
  constants to another process.
- An expansion frees what it allocated when it ends
  ([macroscope.md](../impl/region/macroscope.md)). A mutable table that
  outlives one expansion can hold a value that expansion built, and the next
  reader finds it freed.
- A cache keeps a compiled unit when its inputs fix its expansion
  ([compile.md](../modules/compile.md)). State that expansions change in order
  would make the order an input.

## Facts from several forms

A static definition follows the rule of `def`: a lambda in its expression may
call the name, and a direct read of the name is a compile error
([bindings.md](../bindings.md)). So a unit keeps no accumulator at compile
time, and a macro that gathers facts from several forms takes one of two
shapes:

- It gives each fact a static binding of its own. A record macro binds each
  record's name, and a later macro reads the binding an identifier names.
- It expands to run-time code that gathers the facts when the program starts,
  as a test macro registers each test.

## What an expansion embeds

A template embeds a value through the value node
([modules/macros.md](../modules/macros.md)). The embedded value meets the
sealed test too, for the same three reasons.

## Rejected alternatives

**A static binding that only macro bodies read.** The value would be no
constant at run time, so a program that reflects on a record would define the
record twice.

**A mutable table for the unit.** `begin-for-syntax` allows one today. It
makes expansion order an input of the compile, and it holds freed values.

**A static definition that reads its own earlier binding.** A unit could then
gather facts by defining one name again in each form. The two shapes above
serve that need, and no module under [lib/](../../lib/) gathers facts at
compile time. The read would also make `static` the one definition that breaks
the rule of `def`.

**Evaluating any `def` whose value the compiler can compute.** The set of
static bindings would then follow what an optimizer proves, and a program's
meaning would change with the compiler.
