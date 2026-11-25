# Macro expansion — a closed allocation scope

<!-- audited: 2026-11-24 -->

What a macro expansion frees once it has copied the transformer's result, and the reference the expander gives back.

A macro transformer runs at compile time and builds its expansion as a tree of
runtime `Value`s. The quasiquote template lowers to `list` and `array`
constructor calls that spread each unquote-splice (`quasiquote_to_code`), and
the transformer body runs them to build the output. The expander then
**deep-copies that result into owned `Syntax`** (`Syntax::from_value`). Its
contract forbids any surviving arena pointer, and the `contains_syntax_literal`
debug assert checks that. After the copy, the scratch the transformer built is
dead: the returned tree, its interior nodes, and the scratch a call discards
internally, such as the argument array a spread builds.

The transformer body is ordinary compiled code, so the region solver gives it
the ordinary tail-return treatment. The result region's decref is
**suppressed**, because it is in the return frontier: a function's caller
releases its result through the return convention (Rule 5, `ReturnValue`, in
[rules.md](rules.md)). Tail-flowing native call results take the same
suppression. For an ordinary call that is correct: the caller's
`DecrefValueRegion` consumes the one returned reference, and the cascade
reclaims the rest.

## The expander gives back the result's reference

The expander is the transformer's caller, so after the copy it gives back the
one reference the call handed over with the result, as a compiled caller's
`DecrefValueRegion` does. The result need not live in a region the expansion
minted. A constant the hoist moved into the transformer's unit lives in a
region the transformer's compile minted ([hoist.md](../hoist.md)), and the
transformer returns it from its captures. Left unreleased, that region outlives
every expansion, one per compile of the macro.

## The scope reclaims the scratch

The macro caller is Rust code that keeps only a deep copy. Releasing only the
result-root region would leave every other suppressed or escaped scratch region
holding one unbalanced owner reference. At stdlib scale, thousands of
expansions with several scratch `Pair`s each, that residue would dominate
teardown.

So macro expansion is a **closed allocation scope**. The expander opens it with
`begin_macro_scope`, which starts a per-call log of `(id, generation)` mints on
the heap. The generation stamp makes a recycled id name the right incarnation.
After the copy, `reclaim_macro_scope` balances each surviving region's
**unexplained** references: its RC minus the in-degree it gets from the other
survivors of the scope. Those are the owner references the transformer never
released, and balancing them lets the ordinary cascade (Rule 7) reclaim the
scratch DAG.

The balance is RC-driven, not a blanket free. A region whose count the in-scope
edges explain is left untouched. The in-degree scan reads only the scope's own
survivors, which keeps the close proportional to the scratch. So the scan does
not see an edge from a region outside the scope into the scratch, and a
transformer must not store scratch into a persistent structure.

Three owners live in Rust, where the in-degree scan cannot see them: the
process-root registry, the pinned root region, and the code-hold registry. A
transformer can reach each of them mid-scope, so the close excludes their
regions, and each answers to its own owner
([macroscope.rs](../../../src/value/arena/macroscope.rs)).

The teardown sweep forbids a force-free, because it must reclaim the whole heap
by RC so that leaks stay visible ([rules.md](rules.md)). A closed scope is the
inverse case: it balances its own unreleased references and nothing else.
