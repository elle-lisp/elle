# Macro expansion — a closed allocation scope

<!-- audited: 2026-09-29 -->

How a macro expansion frees every region its transformer made, by balancing references the transformer never released.

## Every region the transformer mints is dead after the copy

A macro transformer runs at compile time and builds its expansion as a tree of
runtime `Value`s. The quasiquote template lowers to `list` / `append` / `array`
constructor calls (`quasiquote_to_code`), and the transformer body executes them
to produce the output. The expander then **deep-copies that result into owned
`Syntax`** (`Syntax::from_value`). The contract of `Syntax::from_value` forbids
any surviving arena pointer (the `contains_syntax_literal` debug assert). After
that, *every region the transformer minted is dead*: the returned tree, its
interior nodes, and the scratch a constructor discards internally (an
`append`-copied segment list) alike.

The transformer body is ordinary compiled code, so the region solver gives it
the ordinary tail-return treatment. The result region's decref is
**suppressed** (it is in the return frontier, escape's return facet), because a
function's caller releases its result through the return convention
([rules.md](rules.md) Rule 5, `ReturnValue`). Tail-flowing native call results
inherit the same suppression. For an ordinary call that is exactly right: the
caller's `DecrefValueRegion` consumes the one returned reference, and the
cascade reclaims the rest.

The macro caller is Rust code that keeps only a *deep copy*. Releasing only the
result-root region would leave every other suppressed or escaped scratch region
holding one unbalanced owner reference. At stdlib scale, thousands of
expansions with several scratch `Pair`s each, that residue dominates teardown.

## The close balances unexplained references

`expand_macro_call` records the regions minted across the transformer call.
Its `begin_macro_scope` opens a per-call `(id, generation)` mint log on the
heap. The generation stamp makes a recycled id name the right incarnation. After
`from_value`, `reclaim_macro_scope` balances each surviving region's
**unexplained** references: its RC minus the in-degree it gets from the other
regions the scope minted. That is the quantity the residue census reports
([rules.md](rules.md)).

Those are exactly the owner references the transformer never released.
Balancing them lets the ordinary cascade (Rule 7) reclaim the whole immutable
scratch DAG. A region whose count its in-scope in-edges explain is left
untouched. The in-degree scan reads only the scope's own regions, which keeps
the close proportional to the scratch. So an edge from a region outside the
scope into scratch is invisible to it, and a transformer must not store scratch
into a persistent structure.

The balance releases only references the scope itself left behind. The teardown
sweep forbids a force-free because it must reclaim the whole heap by RC, so that
leaks stay visible ([rules.md](rules.md)). A closed scope is the inverse case:
it balances its own unreleased references and nothing else.

Three regions a transformer can mint are owned from Rust, where the in-degree
scan cannot see the owner: a process root, the pinned root region, and a code
payload region. The close protects all three, and each answers to its own owner.

## The scope is one arena

`begin_macro_scope` mints the arena and holds one reference on it. Until the
close, every value region the VM mints joins the arena instead of minting
fresh: an allocation slot, a native call's result region, and an environment
value. The wrapped arguments are born in the arena too. Each join takes a
reference that its site releases as usual ([colocation.md](colocation.md)).

So the balance reads one region where it used to read thousands. It finds the
arena unexplained by exactly the references the transformer never released,
plus the scope's own. A mint that must outlive the scope never joins, because a
process root, the root region, and a code payload region mint through
`new_runtime_region` directly. The arena keeps the transformer's garbage until
the close, and the scope bounds that garbage.

An expansion that allocates nothing leaves the arena unmaterialized. The close
then returns its physical id to the free list ([model.md](model.md)).
