(elle/epoch 15)
# audited: 2026-10-28
# A callee called from two file-scope sites keeps its own naming, so neither call frees what the other returned.
# docs/impl/region/mechanism.md
#
# The region walk re-walks a callee it can resolve at each call site, with the
# parameters bound to that site's argument regions. Every callee below has a
# reassigned local that starts as, or takes, a parameter. Were the re-walk's
# naming kept, that local would name the LAST site's argument, and the 1-slot
# gate would suppress that argument's release instead of the parameter's. The
# callee then releases its argument twice, the caller's release of the literal
# frees a region a later allocation took, and the first result reads another
# value's bytes, an io-request, or a freed page.
#
# Each face writes its two call sites at file scope, where the resolution
# holds; a drive wrapped in a `defn` happens to pass without the fix. The read
# runs right after the call and again after churn that recycles freed pages.

(def head "abcdefghijabcdefghijabcdefghijabcdefghijabcdefghij")

(defn churn []
  (let [junk @[]]
    (each i in (range 0 64)
      (push junk (string "junk" i)))
    (length junk)))

# ── 1. The local starts as the parameter ───────────────────────────────
(defn doubled [s n]
  (let [@out s]
    (while (< (length out) n) (assign out (string out out)))
    (slice out 0 n)))

(def first-doubled (doubled "abcdefghij" 50))
(assert (= first-doubled head) "the first call's result is intact")
(doubled "zz" 30)
(churn)
(assert (= first-doubled head)
        "the first call's result survives the second site")

# ── 2. The local takes the parameter by assign ─────────────────────────
(defn taken [s n]
  (let [@out nil]
    (assign out s)
    (while (< (length out) n) (assign out (string out out)))
    (slice out 0 n)))

(def first-taken (taken "abcdefghij" 50))
(assert (= first-taken head) "an assigned parameter's result is intact")
(taken "zz" 30)
(churn)
(assert (= first-taken head)
        "an assigned parameter's result survives the second site")

# ── 3. The arguments are call results ──────────────────────────────────
(def first-built (doubled (string "abcde" "fghij") 50))
(assert (= first-built head) "a built argument's result is intact")
(doubled (string "z" "z") 30)
(churn)
(assert (= first-built head)
        "a built argument's result survives the second site")

# ── 4. Both sites pass the same literal ────────────────────────────────
(def first-same (doubled "abcdefghij" 50))
(def second-same (doubled "abcdefghij" 50))
(churn)
(assert (= first-same head) "the first of two equal calls is intact")
(assert (= second-same head) "the second of two equal calls is intact")

# ── 5. Bounded — each call releases its argument once ──────────────────
# A release the callee takes twice is a UAF above. One it never takes strands
# the literal, one region per call, so the live count grows with the calls.
(def calls 200)
(def @k 0)
(while (< k 20)
  (doubled "abcdefghij" 50)
  (assign k (+ k 1)))
(def before (arena/region-count))
(assign k 0)
(while (< k calls)
  (doubled "abcdefghij" 50)
  (assign k (+ k 1)))
(def grown (- (arena/region-count) before))
(assert (< grown 20)
        (string "a twice-inlined callee strands a region per call: grew by "
                grown " over " calls " calls"))

(println "region-inline-callee-naming-uaf: ok")
