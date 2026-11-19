(elle/epoch 15)
# audited: 2026-11-18
## A Datalog engine: relations of sorted tuples, and variables that grow by rules until a fixpoint, after datafrog 2.0.1.
## docs/stdlib/proof.md
## docs/stdlib/proof/overview.md
##
## A port of datafrog 2.0.1. Datafrog is Copyright (c) Frank McSherry, The
## Rust Project Developers and the Datafrog Developers, and is licensed under
## Apache-2.0 or MIT, at your option.
##
## Loaded via: (def proof ((import "std/proof")))

(fn []
  (def relation ((import "std/proof/relation")))
  (def variable ((import "std/proof/variable") :relation relation))
  (def leapjoin ((import "std/proof/leapjoin") :relation relation))

  (defn from-leapjoin [out v leapers f]
    "Add (F t val) to OUT for each new tuple t of the variable V and each
     value val that LEAPERS agree on."
    (variable:insert out
                     (leapjoin:leapjoin-tuples (variable:recent v) leapers f)))

  {:relation relation:relation
   :elements relation:elements
   :merge relation:merge
   :join relation:join
   :antijoin relation:antijoin
   :map relation:map
   :leapjoin leapjoin:leapjoin
   :iteration variable:iteration
   :variable variable:variable
   :insert variable:insert
   :extend variable:extend
   :changed? variable:changed?
   :complete variable:complete
   :from-join variable:from-join
   :from-antijoin variable:from-antijoin
   :from-map variable:from-map
   :from-leapjoin from-leapjoin
   :extend-with leapjoin:extend-with
   :extend-anti leapjoin:extend-anti
   :filter-with leapjoin:filter-with
   :filter-anti leapjoin:filter-anti
   :prefix-filter leapjoin:prefix-filter
   :value-filter leapjoin:value-filter})
