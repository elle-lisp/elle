(elle/epoch 15)
# audited: 2026-10-21
# The ledger of tests/impl/oracle.lisp, part 2: the stdlib scratch family, each shape's per-call rate beside its crest.
(producer "tests/impl/oracle.lisp")
# The crest's own live-growth row: a list of n pairs, whose high-water mark
# climbs one per element. A floor, voiding every crest when it reads flat.
["crest gauge (live-growth)" :crest :floor 0.5 :class :growth]
# The copy-scratch shapes (probe/scratch.lisp). A per-call rate reads 0
# whether a step's copy dies at the step or at the call's return; the crest
# tells the two apart. Every row is a control.
["walk-rest-array" :objects 0 :root :f1a]
["walk-rest-array" :regions 0 :root :f1a]
["walk-rest-array" :crest 0 :root :f1a]
["take-array" :objects 0 :root :f1a]
["take-array" :regions 0 :root :f1a]
# `take` hands back n pairs, so its crest is the result's own; a tail slice
# stranded per step reads 2.
["take-array" :crest 1 :root :f1a]
["drop-array" :objects 0 :root :f1a]
["drop-array" :regions 0 :root :f1a]
["drop-array" :crest 0 :root :f1a]
["each-string" :objects 0 :root :f1a]
["each-string" :regions 0 :root :f1a]
["each-string" :crest 0 :root :f1a]
["map-unfused" :crest 0 :root :f1a]
["filter-unfused" :crest 0 :root :f1a]
