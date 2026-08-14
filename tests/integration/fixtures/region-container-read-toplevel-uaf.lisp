(elle/epoch 12)
# tests/integration/fixtures/region-container-read-toplevel-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because an over-free in this shape
# SIGSEGVs under --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into
# one shared process where a segfault would take the whole harness down. It is
# exercised by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_container_read_toplevel_uaf`).
#
# WHAT IT PINS — the counted container read is retained by EVERY binder form that
# records it (docs/impl/region/bindings.md § "A whole-value read of a 1-slot
# container takes a counted reference", "Every binder form that records the read
# must emit the retain"). A name bound to a whole-value read of a re-storing
# container borrows a reference the container's next overwrite releases, so the
# reader takes one of its own. The analysis records that read from both binder
# arms of the walk — the `Let` arm and the `Letrec` arm — and hands the container
# its donation on the strength of it. A binder that recorded the read and emitted
# no retain would run both halves against a reference nobody took: the overwrite
# frees the value under the reader, and the reader's own placeholder release
# decrefs it again.
#
# THE SHAPE'S INGREDIENTS (each is load-bearing):
#   * a MODULE-SCOPE reader, so the binder is the file-letrec's rather than a
#     fn-local `let`'s — the fn-local half is pinned by
#     tests/elle/region-reassign-captured-cell-reader.lisp and its guardfree twin;
#   * a HEAP container value, so there is a reference to be dropped at all;
#   * an OVERWRITE between the read and the reader's use, so a donated-away
#     reference is observably gone by the time the reader dereferences;
#   * BOTH realizations of the container — an uncelled `@`-mutable read by no
#     closure, and a capture cell a sibling closure reads — since the reader's
#     obligation is the re-store fact, not the realization;
#   * a read of the reader's CONTENTS (not merely its identity), so the fault
#     lands on the freed page rather than passing a stale pointer along.

# ── (a) uncelled module-scope container, module-scope reader ────────────
(var ua (list 1 2 3))
(def ukeep ua)
(assign ua (list 4 5 6))
(assert (= (first ukeep) 1) "the uncelled reader still names the displaced head")
(assert (= (length ukeep) 3) "the displaced head's whole chain is live")
(assert (= (first ua) 4) "the container reads back what it holds now")

# ── (b) celled module-scope container, module-scope reader ──────────────
# A sibling closure captures `ca`, so its content is re-stored by the cell's own
# update opcode rather than by the compiler's drop-on-overwrite.
(var ca (list 7 8 9))
(def ctouch (fn [] ca))
(def ckeep ca)
(assign ca (list 10 11 12))
(assert (= (first ckeep) 7) "the celled reader still names the displaced head")
(assert (= (length ckeep) 3) "the celled reader's whole chain is live")
(assert (= (first (ctouch)) 10) "the cell reads back what it holds now")

# ── (c) the reader survives a churn that recycles region ids ────────────
# An over-free reads correctly until the freed id is reused, so drive the
# container through enough overwrites to recycle onto it, then read back.
(var ba (list 13 14 15))
(def bkeep ba)
(var i 0)
(while (%lt i 400)
  (assign ba (list i i i))
  (assign i (%add i 1)))
(assert (= (first bkeep) 13)
        "the reader survives 400 overwrites of its container")
(assert (= (length bkeep) 3) "and its chain is walkable after the churn")

(println "region-container-read-toplevel-uaf: ok")
