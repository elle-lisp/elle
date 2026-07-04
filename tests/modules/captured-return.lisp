(elle/epoch 12)
# tests/modules/captured-return.lisp — fixture for
# tests/elle/region-captured-return-move-uaf.lisp.
#
# A closure-as-module that captures a struct at init and exposes accessor
# methods returning it — the shape of `lib/http.lisp`'s `:compress` module
# (`require-compress` returning the captured compress struct). `import-file`
# gives this its OWN compilation unit, so `fetch` has no statically-resolved
# call site in the caller's unit and is NOT inlined — the cross-unit no-inline
# condition the captured-return UAF requires.
(fn [&named cfg]
  (def stored cfg)  # `fetch` RETURNS `stored`, so it escapes via the return facet; `fetch`
  # itself escapes because the sibling methods below capture it and are returned in the module
  # struct. Escape (the authority) marks `stored` returned, so the region solver treats
  # fetch's return as an ESCAPE under the move convention (docs/impl/region-bindings.md).
  (defn fetch []
    stored)
  (defn tag-a []
    (let [c (fetch)]
      (get c :tag)))
  (defn tag-b []
    (let [c (fetch)]
      (get c :tag)))
  (defn tag-c []
    (let [c (fetch)]
      (get c :tag)))
  {:a tag-a :b tag-b :c tag-c})
