# Modules: macros as values

<!-- audited: 2026-11-23 -->

The plan for a macro as a value a module exports, and for how a consumer hands it to the compile that uses it.

[principles.md](principles.md) defines the terms this document uses,
[shape.md](shape.md) states what a load is, and [macros.md](../macros.md)
states how a macro expands today. None of this document is built yet. It says
what changes so that a module may export a macro, and why the change has this
shape. [The macro design](../macros/overview.md) states what a macro becomes
inside one unit.

## A macro is a value

[binding.md](../macros/binding.md) states what a macro value is, and when a
name that holds one heads a macro call. A module's struct therefore exports a
macro as it exports a function. The boot image already carries each prelude
macro as the parts that value holds ([boot.md](../impl/image/boot.md)).

## A compile takes macros as values

`eval` takes an env struct whose symbol-keyed entries bind as names. Each
entry is static to the compile, so an entry that holds a macro heads a macro
call, and an entry that holds a module's struct heads one as `m:name`.
`import/load-syntax`, `import/load-source` and `import/load-file` take the
same env, and a loader of the user's own passes whatever it chooses.

The consumer therefore composes the two steps. It imports the module that
holds the macros, then compiles the unit that uses them with the env:
`(import/load-file path {'my-when m:my-when})`. The compile still reads
nothing outside its unit. The env is an input the compile-input table in
[compile.md](compile.md) already lists, so a macro entry joins the key.
Once a macro is a first-class value, `include-file` is removed, because
`import` then does all that `include-file` does.

## Hygiene across units

A template's identifiers carry the scopes of the unit that defined the macro,
and the transformer compiles in a unit of its own
([syntax.md](../impl/syntax.md)). An expansion in another unit flips its intro
scope on the result as today, so a name the template introduces cannot capture
one at the call site. A free name in the template resolves in the unit that
expands it. So a template names its arguments, the prelude, the standard
library, the primitives, the env's own entries, and the values it embeds.

## A syntax node holds a value

An expansion cannot embed a run-time value today: the expander converts a
transformer's result to syntax and refuses a closure. A new node kind holds a
value. `Syntax::from_value` makes one for a value that has no syntax form, the
analyzer lowers it to a constant that loads that value itself, and an image
dumps it as it dumps any constant. A template therefore reaches its module's
helpers by value: `,helper` in a quasiquote puts the helper's closure in the
expansion, and no name has to resolve in the using unit. A run-time read of a
static binding loads its value through the same node.

The value is shared, not rebuilt. The node holds the one value the transformer
closed over, as a captured binding holds one value. That value is sealed
([static.md](../macros/static.md)), so no expansion shares mutable state
through the node. The node records an edge to the value's region, as a struct
that holds the value would.

## Rejected alternatives

**A compile-time import.** [shape.md](shape.md) rejects it.

**A macro namespace that a run fills.** `(m:when c body)` with `m` bound by an
import in the same unit. The compile holds no value for `m`, because a run
fills that binding. The same head works when `m` is an entry of the env.

**A declaration of macro imports in the file.** A header the loader reads
before it compiles would be syntax that only the default loader honors, and a
loader of the user's own would have to read it too.

**A template closed over the prelude alone.** Without the value node, an
exported macro could name no helper of its own module. It is the first half of
this design and not a shape of its own.
