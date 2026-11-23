# Macros: the plan

<!-- audited: 2026-11-23 -->

The order the macro design lands in, what each part changes, and the decisions still open.

[overview.md](overview.md) states the rule the parts follow, and the documents
beside it state each part.

## The order

| Part | What lands | Needs |
|------|------------|-------|
| Place once | `each` keeps its body in one loop over protocol functions, and a lint reports a template that places an argument twice ([types.md](types.md)) | nothing |
| Syntax as data | Every argument is a syntax object, with the operations that read a node ([syntax.md](syntax.md)) | nothing |
| Macro values | The `:macro` type, and a table that maps a name to a macro value | Syntax as data |
| One walker | The analyzer resolves each head and expands it, and the expander's tree walk goes ([binding.md](binding.md)) | Macro values |
| Static definitions | The form, the sealed test and the operations that read a binding ([static.md](static.md)) | One walker |
| Macro export | A macro in a module's struct, static to the unit that takes it ([modules/macros.md](../modules/macros.md)) | Static definitions |
| Body walks | A transformer expands a subform ([walks.md](walks.md)) | One walker |

Place once comes first because it needs no other part. A loop over a protocol
function sums a wrong answer today (#1558), so that defect goes before it.

Macro values change a representation and no behavior. The part after it then
changes behavior and nothing else.

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
| An atom argument is a syntax object | A macro body that tests a literal argument for truth |
| A local binding shadows a macro | Code that binds a macro's name and still calls the macro in that scope |
| A macro and a variable share one namespace | A unit that defines both under one name |
| A `defmacro` in a body ends with the body | Code after a function that calls a macro the function defined |
| A pattern and a parameter list are not expanded | A parameter list whose first name is a macro's, which is a compile error today |
| `begin-for-syntax` goes | Every unit that uses it |

## Open decisions

- **The names.** `static` and the `meta/` operations are working names.
- **A static definition that reads its own earlier binding.** It would let a
  unit gather facts by defining a name again ([static.md](static.md)). A
  pattern `def` already reads the binding before it
  ([bindings.md](../bindings.md)).
- **A declared type a macro may read.** [types.md](types.md) states why the
  read is sound and why nothing needs it yet.
- **The text of a file as a compile input.** A macro could then build code
  from a schema, where a generator under [tools/](../../tools/) runs today.
  `include-file` already makes a file an input
  ([compile.md](../modules/compile.md)).
- **Which changes an epoch migrates.** A rule can rewrite a
  `begin-for-syntax`. No rule can find a body that tests an atom argument.
