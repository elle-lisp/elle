# Macros: body walks

<!-- audited: 2026-11-23 -->

A transformer may expand a subform until only special forms and calls remain, which a transform over a whole body needs.

This part lands last ([plan.md](plan.md)), and the parts before it are
designed so that it fits. [overview.md](overview.md) states the rule that
admits it.

## What a walk needs

A transform over a whole body rewrites every form the body holds: a change of
calling convention, a derivative, a kernel for another machine. The body holds
macro calls, and a walker that meets one cannot know which of its arguments
run. A walker that guesses repeats, in a user's macro, the defect
[binding.md](binding.md) removes from the expander.

`(meta/expand stx)` answers the subform with every macro call in it expanded,
in the lexical context where the subform stands. The name is a working name.
Every list in the answer is a special form or a call, and the answer keeps the
scopes and spans of the forms it came from.

## The grammar a walker reads

The answer holds the special forms of
[the registry](../../src/hir/analyze/forms/registry.rs), calls, literals and
identifiers. That set becomes part of the language
([spec.md](../spec.md)), because a walker is written against it.

A walker handles the special forms it changes and passes every other node
through `meta/syntax-rebuild` over its walked children
([syntax.md](syntax.md)). A special form added later then passes through a
walker that never heard of it. A walker still has to know which positions of a
special form bind a name, so a new binding form is a change every walker
meets.

## One pass is enough

A name exists from its `def` onward ([bindings.md](../bindings.md)), so no
form of a body reads a definition that follows it. The expansion of a body
therefore reads its forms in order and binds each definition as it passes. A
language whose body definitions see each other has to find every definition
before it expands any of them.

## The analyzer expands the answer again

The transformer returns a tree, and the analyzer analyzes it as it analyzes
any expansion. A macro call the walker built is expanded then. A static
definition inside the walked subform is evaluated a second time, and gives the
same value, because expansion is pure
([compile.md](../modules/compile.md)).

## What it costs

- The special forms become an interface. Each one a release adds or changes is
  a change to the language.
- The order of expansion becomes visible. A subform expands while its
  enclosing macro runs, and the fuel that expansion spends is the enclosing
  macro's.
- A walked subform is expanded twice, so a walker over a large body costs a
  second expansion of that body.

## Rejected alternatives

**A walker that expands by hand.** It calls each macro's transformer itself
and handles the binding forms itself. It is the expander written again, by
every author of a walk.

**A stop list.** The caller would name macros to leave unexpanded, as
Racket's `local-expand` allows. A walker that needs to see one macro whole
shadows that macro's name instead.
