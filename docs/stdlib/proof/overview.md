# proof

<!-- audited: 2026-11-18 -->

The submodules behind [proof.lisp](../../../lib/proof.lisp): relations and their joins, variables and the fixpoint, and the leapjoin.

[proof.md](../proof.md) holds what a caller of the module sees. This file
holds how the module divides, and the reference it follows.

| File | Purpose |
|------|---------|
| [relation.lisp](../../../lib/proof/relation.lisp) | A relation, the merge of two, the gallop and binary search over a sorted array, and the joins, antijoin and map over relations |
| [variable.lisp](../../../lib/proof/variable.lisp) | The iteration, a variable's three stages, the round `changed?` ends, and the rules that write a variable |
| [leapjoin.lisp](../../../lib/proof/leapjoin.lisp) | The leapjoin and its six leapers |

The port follows datafrog 2.0.1 function for function, and keeps its
license notice, Apache-2.0 or MIT. A variable holds its three stages, the
stable batches, the new tuples and the tuples to add, in one mutable struct
that the iteration and every rule share. A relation's tuples are an
immutable array, because the gallop and the binary search walk it by index.

A submodule takes the submodules it needs as arguments:

```lisp
(def relation ((import "std/proof/relation")))
(def variable ((import "std/proof/variable") :relation relation))
(def leapjoin ((import "std/proof/leapjoin") :relation relation))
(assert (= [1 2 3] (relation:elements (relation:relation [3 1 2 1]))))
```
