(elle/epoch 12)
# audited: 2026-09-29
# A binary append or concat copies each chunk in bulk, measured against a text append of the same size.
# docs/testing.md
#
# concat-linear.lisp covers the string side. `append` and `concat` route
# through `push-all` (src/core.lisp), which hands a bytes source to
# `%bytes-push` whole, and `%bytes-push` appends a whole bytes value in one
# copy, as `%string-push` does a string. The Rust test
# `bytes_push_bulk_appends_bytes_value` (primitives::intrinsics::tests) pins
# the bulk form. HTTP/2 frame read-exact accumulates each body with `append`.
#
# The counter-factual: a `push-all` that walks a bytes source element by
# element makes one interpreted `%bytes-push` per byte. A 320 KiB text append
# then takes under a millisecond and the same binary append about half a
# second.
#
# The text append of the same size is the control. A wall-clock bound cannot
# tell a per-byte binary path from a runner that lost its CPU inside the
# measured window; the ratio between the two appends can. docs/testing.md
# holds the argument.

# Time both thunks over `rounds` alternating rounds and return [control subject]
# — the smallest elapsed each one reached. Alternating samples the same stretch
# of machine, and the minimum discards a round the scheduler stalled, so one
# starved run cannot decide the comparison.
(defn best-of [rounds control subject]
  (let [@a nil
        @b nil
        @i 0]
    (while (< i rounds)
      (let [ca (second (time/elapsed control))
            cb (second (time/elapsed subject))]
        (when (or (nil? a) (< ca a)) (assign a ca))
        (when (or (nil? b) (< cb b)) (assign b cb)))
      (assign i (+ i 1)))
    [a b]))

# A 20 KiB immutable bytes chunk (the shape read-exact appends per socket read),
# and the 20 KiB text chunk that is its control.
(def chunk
  (let [@b (@bytes)
        @i 0]
    (while (< i 20000)
      (%bytes-push b (bit/and i 0xff))
      (assign i (+ i 1)))
    (freeze b)))
(def text
  (let [@s (@string)
        @i 0]
    (while (< i 20000)
      (%string-push s "x")
      (assign i (+ i 1)))
    (freeze s)))
(assert (= (length text) (length chunk))
        "the control chunk and the measured chunk are the same size")

# Accumulate ~2 MiB by appending the chunk 100 times, exactly as read-exact
# accumulates a large body from many socket reads. A per-byte push makes this
# ~2M interpreted %bytes-push calls, whole seconds; the bulk path makes it ~100
# memcpys, milliseconds. `make` hands back a fresh
# accumulator per run, so every timed round does the same work from scratch.
(defn accumulate [make chunk]
  (let [@acc (make)
        @i 0]
    (while (< i 100)
      (append acc chunk)
      (assign i (+ i 1)))
    acc))
(def binary (fn () (accumulate (fn () (@bytes)) chunk)))
(def textual (fn () (accumulate (fn () (@string)) text)))

(def buf (freeze (binary)))
(assert (= (length buf) 2000000) "accumulated the full 2 MiB")
(assert (= (slice buf 0 4) (bytes 0 1 2 3)) "content preserved at head")
(assert (= (slice buf 20000 20004) (bytes 0 1 2 3))
        "content preserved across chunks")

# Both appends walk the same push-all branch over the same byte count, so they
# cost about the same and the multiple is slack, not a budget: the per-byte path
# is ~100 chunk-copies where the bulk path is one.
(def [control measured] (best-of 5 textual binary))
(assert (< measured (* 8 control))
        (concat "binary append must be bulk; 100×20KiB append took "
                (string measured) "s against " (string control)
                "s for the same-size text append (per-byte regression)"))

# concat over many binary chunks (the `(apply concat ...)` body-assembly path)
# must be bulk too.
(def parts
  (let [@ps @[]
        @i 0]
    (while (< i 100)
      (push ps chunk)
      (assign i (+ i 1)))
    (freeze ps)))
(def joined (apply concat parts))
(assert (= (length joined) 2000000) "apply concat over 100 binary chunks")

(println "bytes-linear ok: |buf|=" (length buf) " append took " measured
         "s against " control "s for text")
