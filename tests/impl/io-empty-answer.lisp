(elle/epoch 15)
# audited: 2026-11-17
# A file call whose answer is empty releases no region it never built.
# docs/impl/io/inflight.md
#
# The trap: a completion mints its answer's region when it asks for an
# allocator, and the empty list is an immediate. So an empty listing asks,
# builds nothing, and leaves an id that names no region.
#
# The counter-factual: a handover that releases that id anyway passes every
# assertion on the answer. It panics under debug assertions, and a release
# build only counts it, so the `arena/over-frees` reading is what fails there.

(def over-frees (arena/over-frees))

(with-temp-dir dir
  (assert (= () (file/ls dir)) "an empty directory lists as the empty list")
  (let [empty (path/join dir "empty")]
    (file/write empty "")
    (assert (= () (file/lines empty))
            "an empty file reads as the empty list of lines")))

(assert (= (arena/over-frees) over-frees)
        "no empty answer released a region it never built")
