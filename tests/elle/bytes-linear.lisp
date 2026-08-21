(elle/epoch 12)
# tests/elle/bytes-linear.lisp — binary concat/append must be BULK, not per-byte.
#
# The binary sibling of concat-linear.lisp (which pins string concat).
# `append`/`concat` route through `push-all` (src/core.lisp), which bulk-appends
# a STRING source in one shot (%string-push copies all its bytes at once) but
# used to walk a BYTES source element-by-element — one %bytes-push per byte
# through the interpreter. Identical-sized binary payloads then cost orders of
# magnitude more than text: a 320 KiB text append is sub-millisecond, the same
# binary append ran ~0.5 s. This is the HTTP/2 body-copy hot path — frame
# read-exact accumulates the body with `append` — so a bulk binary path is
# required, not optional.
#
# The fix gives %bytes-push a whole-bytes bulk form (mirroring %string-push);
# push-all then bulk-appends bytes sources too. Pinned in Rust by
# `bytes_push_bulk_appends_bytes_value` (primitives::intrinsics::tests).

# A 20 KiB immutable bytes chunk (the shape read-exact appends per socket read).
(def chunk
  (let [@b (@bytes)]
    (def @i 0)
    (while (< i 20000)
      (%bytes-push b (bit/and i 0xff))
      (assign i (+ i 1)))
    (freeze b)))

# Accumulate ~2 MiB by appending the chunk 100 times, exactly as read-exact
# accumulates a large body from many socket reads. Pre-fix (per-byte push) this
# is ~2M interpreted %bytes-push calls and runs into whole seconds; post-fix it
# is ~100 bulk memcpies and completes in milliseconds.
(def t0 (clock/monotonic))
(def buf
  (let [@acc (@bytes)]
    (def @i 0)
    (while (< i 100)
      (append acc chunk)
      (assign i (+ i 1)))
    (freeze acc)))
(def elapsed (- (clock/monotonic) t0))

(assert (= (length buf) 2000000) "accumulated the full 2 MiB")
(assert (= (slice buf 0 4) (bytes 0 1 2 3)) "content preserved at head")
(assert (= (slice buf 20000 20004) (bytes 0 1 2 3))
        "content preserved across chunks")

# A generous bound: the per-byte path takes seconds and blows it; the bulk
# path takes milliseconds and clears it with wide margin.
(assert (< elapsed 0.5)
        (concat "binary append must be bulk; 100×20KiB append took "
                (string elapsed) "s (per-byte regression)"))

# concat over many binary chunks (the `(apply concat ...)` body-assembly path)
# must be bulk too.
(def parts
  (let [@ps @[]]
    (def @i 0)
    (while (< i 100)
      (push ps chunk)
      (assign i (+ i 1)))
    (freeze ps)))
(def joined (apply concat parts))
(assert (= (length joined) 2000000) "apply concat over 100 binary chunks")

(println "bytes-linear ok: |buf|=" (length buf) " append took " elapsed "s")
