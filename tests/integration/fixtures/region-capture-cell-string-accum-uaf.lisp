(elle/epoch 12)
# tests/integration/fixtures/region-capture-cell-string-accum-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because it SIGSEGVs under
# --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into one shared
# process where a segfault would take the whole harness down. It is exercised by
# the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_capture_cell_string_accum_uaf`), #[ignore]'d RED until the over-free
# is fixed.
#
# WHAT IT REPRODUCES — the STRING sibling of region-fold-closure-arg-uaf (that
# one over-frees a threaded/cell-held CLOSURE; this one over-frees a cell-held
# STRING, same trace context).
#   A helper builds a string by reassigning a `@`-capture cell in a loop
#   (`(assign out (string out …))`) and RETURNS `out`. The region backing the
#   returned string reaches refcount 0 as the activation unwinds — its
#   DecrefValueRegion frees the string — and the caller, which reads the returned
#   value one form later (here `(string "dial-" slug)`), derefs the freed page:
#
#     [guardfree] SIGSEGV — use-after-free
#     free site: DecrefValueRegion of string (runtime region N)   <- the `out` return
#     context:   UpdateCapture
#
# ISOLATING INGREDIENTS (dropping any ONE makes it guardfree-clean):
#   * the accumulation is a LOOP over the capture cell (a single `(assign out …)`
#     is clean — the loop-carried reassignment is what strands the region);
#   * `safe-uri` is a FUNCTION whose result a CALLER consumes (the identical body
#     inlined at top level is clean — the activation boundary is load-bearing);
#   * the returned string is READ after return (built into another string / put
#     in a struct); returning it untouched is clean.
#
# THE CONTRAST — a capture cell that is NOT reassigned in a loop, or a builder
# that returns into the SAME scope that consumes it, is guardfree-clean, so this
# isolates the loop-reassigned-cell return lifetime, not string building in
# general.
#
# ORIGIN — this is mu's `_safe-uri` (lib/cont/repo.lisp) and its twin `_slug`
# (lib/cont/config.lisp): a URI→ref-safe slug accumulated in `@out @""`, then
# consumed into a branch name. --trace=guardfree on the mu suites points at the
# identical free site (repo.lisp:634:5 / config.lisp:261:5, context UpdateCapture)
# for dial-owner-git, repo, and adopt-config; without guardfree the freed region
# is silently reused, so the slug reads back as garbage (adopt-config saw an empty
# `:branch`) — a data-corruption bug even when it does not crash.
#
# WHEN FIXED — the region solver keeps a loop-reassigned capture cell's returned
# region live across the caller's read; this exits 0 under guardfree — un-#[ignore]
# the pin then. This is distinct from the already-fixed struct write-path over-free
# (`struct_put_with_rebind`); this is the capture-cell string return path.

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
# sequence and can hide the fault. This is a UAF guard, not a value test. It
# faults on the first rep; the loop only makes it robust to a partial fix.
(defn drive [reps]
  (def @c 0)
  (while (< c reps)
    (build-branch "a-b-c")
    (assign c (+ c 1))))

(drive 200)
(println "region-capture-cell-string-accum-uaf: ok")
