(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-string-accum-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because an over-free in this shape
# SIGSEGVs under --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into
# one shared process where a segfault would take the whole harness down. It is
# exercised by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_capture_cell_string_accum_uaf`).
#
# WHAT IT PINS — the STRING sibling of region-fold-closure-arg-uaf (that one
# exercises a threaded/cell-held CLOSURE's lifetime; this one a cell-held
# STRING's). A helper builds a string by reassigning a `@`-capture cell in a
# loop (`(assign out (string out …))`) and RETURNS `out`; the caller reads the
# returned value one form later (here `(string "dial-" slug)`). Green pins that
# the loop-reassigned capture cell's returned region stays live across the
# caller's read.
#
# THE SHAPE'S INGREDIENTS (each is load-bearing for reaching that lifetime):
#   * the accumulation is a LOOP over the capture cell (the loop-carried
#     reassignment is what exercises the return lifetime);
#   * `safe-uri` is a FUNCTION whose result a CALLER consumes (the activation
#     boundary is load-bearing — the identical body inlined at top level takes a
#     different route);
#   * the returned string is READ after return (built into another string / put
#     in a struct).
#
# ORIGIN — this is mu's `_safe-uri` (lib/cont/repo.lisp) and its twin `_slug`
# (lib/cont/config.lisp): a URI→ref-safe slug accumulated in `@out @""`, then
# consumed into a branch name (the dial-owner-git, repo, and adopt-config
# suites). Distinct from the struct write-path (`struct_put_with_rebind`,
# pinned by region_struct_mut_put_heap_key_uaf); this is the capture-cell string
# return path.

(defn safe-uri [uri]
  # Mirrors mu lib/cont/repo.lisp `_safe-uri`: slug the URI into a `@`-cell.
  (let [@out @""]
    (each i in (range (length uri))
      (let [c (slice uri i (+ i 1))]
        (assign out (string out c))))
    out))

(defn build-branch [uri]
  # Mirrors dial-owner-setup: the accumulated slug is READ one form after
  # `safe-uri` returns — the deref that faults on the freed page.
  (let [slug (safe-uri uri)]
    (string "dial-" slug)))

# Discard the result on purpose — comparing/printing it changes the allocation
# sequence and can hide an over-free. This is a UAF guard, not a value test; the
# reps keep it sensitive to a lifetime that only breaks under region-id churn.
(defn drive [reps]
  (def @c 0)
  (while (< c reps)
    (build-branch "a-b-c")
    (assign c (+ c 1))))

(drive 200)
(println "region-capture-cell-string-accum-uaf: ok")
