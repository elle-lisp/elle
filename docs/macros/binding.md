# Macros: a macro is a binding

<!-- audited: 2026-11-23 -->

A name binds a macro as it binds any value, and a form is a macro call when its head resolves to a static binding that holds a macro.

[overview.md](overview.md) defines the terms this document uses.
[static.md](static.md) states how a program makes a static binding of its own.

## One namespace

Macros, variables and special forms share the lexical scope. A `let`, a `def`
or a parameter that takes a macro's name shadows the macro, and a `defmacro`
in a body binds from its own form onward, as a `def` does
([bindings.md](../bindings.md)). So a macro defined in a function body ends
with that body.

A special form is a binding of the initial scope, as a primitive is. Code may
shadow it: in `(let [if f] (if a b c))` the inner form calls `f`. A template's
`if` still means the special form, because a template's identifiers resolve
where the macro was defined ([macros.md](../macros.md)). One rule therefore
covers every name, and no list of reserved words exists.

## What makes a macro call

A list form is a macro call when its head resolves to a static binding whose
value is a macro. The head resolves through the lookup that resolves a
variable ([the analyzer](../../src/hir/analyze/AGENTS.md)), so a pattern or a
parameter list, which holds no head, is never expanded.

The graft admits a call by the same test: an immutable binding that no code
writes, whose value the compile holds ([graft.md](../impl/graft.md)). The
graft changes no meaning, and this rule does. So the language states which
bindings are static, and an implementation may not find more:

| Static binding | The value the compile holds |
|----------------|-----------------------------|
| A special form, a primitive, a prelude macro, a standard library export, a binding an embedder registers | What the instance holds for it |
| An entry of the env an `eval` or a loader takes | The entry's value |
| A static definition | The value it computed ([static.md](static.md)) |
| An immutable binding whose initializer is a reference to a static binding | That binding's value |
| An immutable binding whose initializer reads a field of a static struct, as `m:name` does | The field's value |

No other binding is static. A parameter is not, a binding that code assigns is
not, and neither is a binding whose initializer runs when the program runs. A
head written `m:when` is a macro call when `m` is static and its `when` field
holds a macro.

## A macro is a value

A macro has a type of its own: `type-of` answers `:macro`. The value holds the
parameter lists, the template and the compiled transformer. In value position,
a macro's name evaluates to the macro, so a program stores it, passes it,
returns it and exports it in a module's struct.

A call whose head holds a macro only when the program runs raises an error that
names the macro. The transformer never runs on evaluated arguments, because
every binding is resolved before any code runs.

## What a transformer's body resolves

A transformer's body resolves its free names where the macro is defined, among
the static bindings in scope there. A free name that resolves to a binding
which is not static is a compile error. A macro's helper is therefore a static
definition, or an entry of the env the unit was compiled with.

## A phase is an eval

Whatever a compile receives as a value is static to it, and whatever its own
run-time code builds is not. A program that needs a value at compile time
computes it in a static definition, or computes it in one unit and hands it to
the `eval` of the next. No form names a phase, and no binding exists in two
phases with two values.

## Rejected alternatives

**A binding form decides.** Only `defmacro` and a `let`-style macro form
would bind a macro, as in Racket. A macro taken from a module's struct would
then need a second form before use, and an alias would not work.

**A macro applied when the program runs.** A head that a parameter fills
would expand at run time. Bindings are resolved at analysis time, so no
compile could produce that code.

**Reserved special form names.** A binder that names a special form would be
a compile error. It keeps a reader from meeting a shadowed `if`, at the price
of a second rule for some thirty names.
