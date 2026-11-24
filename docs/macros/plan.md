# Macros: the plan

<!-- audited: 2026-11-24 -->

The order the macro design lands in, what each part changes, and the decisions still open.

[overview.md](overview.md) states the rule the parts follow, and the documents
beside it state each part.

## The order

| Part | What lands | Needs |
|------|------------|-------|
| Place once | `each` keeps its body in one loop that dispatches around it, `if-let` places its else form once, and `W006` reports a template that places an argument twice ([types.md](types.md)) | nothing |
| Syntax as data | Every argument is a syntax object, with the operations that read a node ([syntax.md](syntax.md)) | nothing |
| Macro values | The `:macro` type, and a table that maps a name to a macro value | Syntax as data |
| One walker | The analyzer resolves each head and expands it, the expander's tree walk goes, and `syntax/same-binding?` compares two identifiers ([binding.md](binding.md)) | Macro values |
| Static definitions | The form, the sealed test and `syntax/static-value` ([static.md](static.md)) | One walker |
| Macro export | A macro in a module's struct, static to the unit that takes it ([modules/macros.md](../modules/macros.md)) | Static definitions |
| Body walks | A transformer expands a subform ([walks.md](walks.md)) | One walker |

Place once comes first because it needs no other part, and it is built.

Macro values change a representation and no behavior. The part after it then
changes behavior and nothing else.

## Syntax as data

The part lands in five steps, each one usable before the next:

| Step | What lands |
|------|------------|
| The names | Epoch 19 moves every syntax operation under `syntax/`, and the bare aliases go |
| Reading a node | `syntax/kind`, `syntax/children`, `syntax/rebuild`, `syntax/span` and `syntax/error` |
| A datum keeps its kind | `syntax/->datum` and `read` answer a struct for a struct literal and a set for a set literal |
| Atom arguments | A transformer receives an atom argument as a syntax object |
| Patterns over syntax | `match` destructures a syntax object |

The names, reading a node and a datum that keeps its kind are built.

`if-let` gives its else form every name ([control.md](../control.md)). Its
expansion reads each pattern's kind and children to find the names it binds,
because a struct pattern read as a datum is a list that starts with `struct`.

## One walker

The expander's tree walk does more than expand, and each of its other jobs
moves into the analyzer: quasiquote lowering, the mutable string literal, the
splice of an include, `syntax-case`, the `macro?` and `expand-macro` forms,
and keyword learning. A body's forms are classified
after their heads expand, because `defn` is a macro. The analyzer runs
transformers, so it takes the VM an expansion runs on
([pipeline.md](../pipeline.md)).

While both walks exist, a harness compiles the standard library, every module
under [lib/](../../lib/) and the language suite through each, and compares the
HIR. The claims [overview.md](overview.md) lists are the differences it may
show. The harness goes when the old walk goes.

## What changes for a program

| Change | A program that meets it |
|--------|-------------------------|
| The syntax operations take `syntax/` names | Every call of `syntax->datum`, `datum->syntax`, `syntax-e` and the rest |
| `read` and `syntax/->datum` answer a struct or a set for its literal | A program that walks a datum for a `struct` or `set` head |
| An atom argument is a syntax object | A macro body that tests a literal argument for truth |
| `if-let`'s else form sees every name | An else form that reads an outer binding of a name the `if-let` binds |
| A local binding shadows a macro | Code that binds a macro's name and still calls the macro in that scope |
| A macro and a variable share one namespace | A unit that defines both under one name |
| A `defmacro` in a body ends with the body | Code after a function that calls a macro the function defined |
| A pattern and a parameter list are not expanded | A parameter list whose first name is a macro's, which is a compile error today |
| `begin-for-syntax` goes | Every unit that uses it |

A rename rule carries each syntax operation to its new name, and one rule
rewrites each `begin-for-syntax` definition to a static definition
([epochs.md](../epochs.md)). The other seven changes have no rule. The epoch's
summary names each one, and [macros.md](../macros.md) states what a file of an
older epoch reads instead. No rule can find a walk over a datum, a body that
tests an atom argument or an else form that reads an outer name, so those
changes break without an error.

## A target no part may break

A macro may one day read a declared type ([types.md](types.md)). No part
builds the read, and no part may move a declared type out of reach of the
point where a form expands.

## Not planned

The text of a file is not a compile input. A program that builds code from a
schema reads the schema at run time, where a generator under
[tools/](../../tools/) runs today. Run-time code generation is the work that
comes first.

## Open decisions

- **The names.** `static` and `syntax/static-value` are working names.
