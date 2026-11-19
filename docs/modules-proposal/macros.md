# Modules: macros as values

<!-- audited: 2026-11-19 -->

How a macro becomes a value a module exports, and how a consumer hands it to the compile that uses it.

[The proposal](../modules-proposal.md) defines the terms this document uses,
[shape.md](shape.md) states what a load is, and [macros.md](../macros.md)
states how a macro expands today. This document says what changes so that a
module may export one, and why the change has this shape.

## A macro is a value

A macro is a value of its own type: `type-of` answers `:macro`, and `macro?`
answers true for it. It holds what a definition holds today: the parameter
lists, the template, and the compiled transformer, or nil until the first
expansion compiles it. The boot image already carries each prelude macro in
that shape ([boot.md](../impl/image/boot.md)).

`(defmacro name params body)` makes the value, registers it for the rest of
the unit, and binds `name` to it where a `def` would bind. So a module's struct
exports a macro as it exports a function, and a `defmacro` inside a `fn` body
binds a local as `defn` does. A macro and a closure are distinct types, so a
call site cannot take one for the other.

## A compile takes macros as values

`eval` takes an env struct whose symbol-keyed entries bind as names. An entry
whose value is a macro binds that name as a macro for the unit instead, beside
the prelude's macros and the unit's own. `import/load-syntax`,
`import/load-source` and `import/load-file` take the same env, and a loader of
the user's own passes whatever it chooses.

The consumer therefore composes the two steps. It imports the module that
holds the macros, then compiles the unit that uses them with the env:
`(import/load-file path {'my-when m:my-when})`. The compile still reads
nothing outside its unit. The env is an input the compile-input table in
[compile.md](compile.md) already lists, so a macro entry joins the key.
`include-file` stays the textual way, and a file that wants macros from
another file with no consumer's help includes it.

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
expansion, and no name has to resolve in the using unit. `defmacro` binds its
name through the same node.

The value is shared, not rebuilt. The node holds the one value the transformer
closed over, as a captured binding holds one value, so a mutable value embedded
this way is shared by every expansion. The node records an edge to the value's
region, as a struct that holds the value would.

## Rejected alternatives

**A compile-time import.** [shape.md](shape.md) rejects it.

**A macro namespace at the head of a form.** `(m:when c body)` with `m` an
imported module. The expander knows no `m` while it expands, because `m` is a
binding a run fills.

**A declaration of macro imports in the file.** A header the loader reads
before it compiles would be syntax that only the default loader honors, and a
loader of the user's own would have to read it too.

**A template closed over the prelude alone.** Without the value node, an
exported macro could name no helper of its own module. It is the first half of
this design and not a shape of its own.
