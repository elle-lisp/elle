(elle/epoch 15)
# audited: 2026-11-10
# A closure released on a branch arm frees what it adopted, so that arm must not read it.
# docs/impl/region/adopt.md
# docs/impl/region/compensate.md
#
# `dl` captures `seen` and adopts it, so the closure's release frees `seen` with
# it. One arm calls `dl` and holds the closure's release. Every sibling arm
# releases the closure at its head, since it never names `dl`. A sibling arm
# that then writes `seen` writes a freed table.
#
# The counter-factual: the lifetime obligation read one release point per
# region. `seen`'s last use and the closure's release both sit in the calling
# arm, so the obligation admitted the adopt, and the head release on the
# writing arm freed `seen` under the write. Under the sidecar's guardfree that
# write faults.
#
# Each subject runs through a higher-order caller. A direct call is grafted,
# and the graft deletes the arm that a constant argument never takes.

# ── subjects ─────────────────────────────────────────────────────────

(defn arm-let [coll]
  (let [seen @{}]
    (let [dl (fn (lst acc)
               (put seen 1 true)
               acc)]
      (if (array? coll)
        (begin
          (put seen 2 true)
          1)
        (dl coll ())))))

(defn arm-letrec [coll]
  (let [seen @{}]
    (letrec [dl (fn (lst acc)
                  (put seen 1 true)
                  acc)]
      (if (array? coll)
        (begin
          (put seen 2 true)
          1)
        (dl coll ())))))

(defn arm-letrec-local [coll]
  (let [seen @{}]
    (letrec [dl (fn (lst acc)
                  (put seen 1 true)
                  acc)]
      (if (array? coll)
        (let [result @[]]
          (put seen 2 true)
          result)
        (dl coll ())))))

(defn arm-cond [coll]
  (let [seen @{}]
    (let [dl (fn (lst acc)
               (put seen 1 true)
               acc)]
      (cond
        (array? coll)
          (begin
            (put seen 2 true)
            1)
        (string? coll) 2
        true (dl coll ())))))

(defn arm-match [coll]
  (let [seen @{}]
    (let [dl (fn (lst acc)
               (put seen 1 true)
               acc)]
      (match (type-of coll)
        :@array
          (begin
            (put seen 2 true)
            1)
        _ (dl coll ())))))

# The admitting twin: the sibling arm names neither `dl` nor `seen`, so the
# closure's release there frees the pair on a path that never reads them.
(defn arm-dead [coll]
  (let [seen @{}]
    (let [dl (fn (lst acc)
               (put seen 1 true)
               acc)]
      (if (array? coll) 1 (dl coll ())))))

# The stdlib's `distinct` binds a list walker over a captured table and calls
# it only for a list, so an array takes an arm that writes the table directly.
(defn distinct-array [coll]
  (length (distinct coll)))

(defn run [f arg]
  (def @i 0)
  (def @out nil)
  (while (< i 200)
    (assign out (f arg))
    (assign i (+ i 1)))
  out)

(def rows
  [["arm-let          " arm-let @["b" "a"] 1 (list 1 2) ()]
   ["arm-letrec       " arm-letrec @["b" "a"] 1 (list 1 2) ()]
   ["arm-letrec-local " arm-letrec-local @["b" "a"] @[] (list 1 2) ()]
   ["arm-cond         " arm-cond @["b" "a"] 1 "ab" 2]
   ["arm-match        " arm-match @["b" "a"] 1 (list 1 2) ()]
   ["arm-dead         " arm-dead @["b" "a"] 1 (list 1 2) ()]
   ["distinct-array   " distinct-array @[2 1 2] 2 (list 2 1 2) 2]])

(each row in rows
  (let [[name f writing want-w other want-o] row]
    (assert (= (run f writing) want-w) (string name " on the writing arm"))
    (assert (= (run f other) want-o) (string name " on the calling arm"))
    (println "  " name " ok")))

(println "region-arm-release-adopt-uaf: ok")
