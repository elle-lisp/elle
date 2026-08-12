(elle/epoch 12)
# tests/integration/fixtures/region-cell-forward-chain-uaf.lisp
#
# Quarantined here — NOT under tests/elle/ — because an over-free in this shape
# SIGSEGVs under --trace=guardfree, and `make smoke` globs tests/elle/*.lisp into
# one shared process where a segfault would take the whole harness down. It is
# exercised by the guardfree subprocess pin in tests/integration/elle_scripts.rs
# (`region_cell_forward_chain_uaf`). The leak and read-back faces of the same
# shape live in tests/elle/region-cell-forward-chain.lisp.
#
# WHAT IT PINS — the soundness half of the forwarding CHAIN
# (docs/impl/region/bindings.md § "A chain of forwarding edges hands one
# reference along, so the fold follows it whole"). Two sequential loops over one
# reassigned mutable make functionalization chain the versions
# (`last#2 <- last#1 <- last#0`), and each link's cell claims the one reference
# the chain forwards. Exactly one link may release it: the one holding it when
# its own slot is overwritten, or the last link at its scope demise.
#
# THE SHAPE'S INGREDIENTS (each is load-bearing):
#   * TWO sequential loops over the SAME binding — one loop gives a two-version
#     chain no middle cell sits in, which is the already-settled case;
#   * a HEAP init, so the forwarded reference names a region at all (a `nil`
#     init carries none and the chain is inert);
#   * a heap value assigned in BOTH loops, so both links carry a cell;
#   * iteration counts that vary per call, so a loop that runs zero times hands
#     its predecessor's content straight on;
#   * a FUNCTION boundary, so the cells are fn-local (a module-scope cell is
#     freed by the file-letrec teardown and never reaches this edge);
#   * enough reps for region ids to recycle onto a freed one — an over-free is
#     state-dependent and reads correctly until the id is reused.

(defn chain [n m]
  (var last (array 0 0))
  (var i 0)
  (while (< i n)
    (assign last (array i 7))
    (assign i (+ i 1)))
  (var j 0)
  (while (< j m)
    (assign last (array j 9))
    (assign j (+ j 1)))
  (get last 1))

# The keeper face: every value the loops produce also gets a runtime-counted
# funnel store, so a value released twice is freed under a live holder and the
# read below walks a reclaimed page.
(defn chain-keep [n]
  (var out @[])
  (var last (array 0 0))
  (var i 0)
  (while (< i n)
    (assign last (array i 7))
    (%array-push out last)
    (assign i (+ i 1)))
  (var j 0)
  (while (< j n)
    (assign last (array j 9))
    (%array-push out last)
    (assign j (+ j 1)))
  (get (get out (- (length out) 1)) 0))

# A three-link chain: a third sequential loop extends the forwarding chain by
# one more middle cell, so the "every link but the last forwards" rule is
# exercised at a length the two-loop shape cannot reach.
(defn chain3 [n]
  (var last (array 0 0))
  (var i 0)
  (while (< i n)
    (assign last (array i 1))
    (assign i (+ i 1)))
  (var j 0)
  (while (< j n)
    (assign last (array j 2))
    (assign j (+ j 1)))
  (var k 0)
  (while (< k n)
    (assign last (array k 3))
    (assign k (+ k 1)))
  (get last 1))

(defn drive [reps]
  (def @c 0)
  (while (< c reps)
    # Vary the two loops' trip counts independently, including zero, so the
    # forwarded reference reaches every link with and without an overwrite.
    (chain (mod c 3) (mod c 4))
    (chain-keep (+ 1 (mod c 3)))
    (chain3 (mod c 3))
    (assign c (+ c 1))))

(drive 400)
(println "region-cell-forward-chain-uaf: ok")
