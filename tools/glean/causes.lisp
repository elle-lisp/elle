(elle/epoch 18)
# audited: 2026-11-23
# The causes that claim each name of a census row, and the tally of their claims.
# tools/glean/census.md

(fn []
  (def {:spelled spelled} ((import "./rows")))
  (def {:view view} ((import "./points")))

  (def causes
    [:constructor :hoisted :unruled :error-path :grafted :unnamed :captures
     :closure-result])
  (def raised [:constructor :hoisted :unruled :error-path])

  # The fact keys whose tuples are a name alone.
  (def bare |:free :captured :immutable :immediate|)

  (defn entries [row]
    "Each name of ROW as [judgment side name], in the row's order."
    (def out @[])
    (each j in [:released :refused]
      (each s in [:hand :glean]
        (each n in (get (get row j) s)
          (push out [j s n]))))
    out)

  (defn held [facts]
    "The set of the names FACTS holds, spelled."
    (def out @[])
    (each [k tuples] in (pairs facts)
      (each t in tuples
        (if (contains? bare k)
          (push out (spelled t))
          (each x in t
            (push out (spelled x))))))
    (set ;out))

  (defn passed [facts start]
    "START, a set of names, with each value they pass into along FACTS's passes."
    (var names start)
    (var changed true)
    (while changed
      (assign changed false)
      (each [y x _] in (get facts :pass [])
        (when (and (contains? names (spelled y))
                   (not (contains? names (spelled x))))
          (assign names (union names (set (spelled x))))
          (assign changed true))))
    names)

  (defn raised-names [fun noise]
    "From each cause raised as NOISE in FUN to the set of names it claims."
    (def mine (filter (fn [e] (= (get e :id) (get fun :id))) noise))
    (from-pairs (map (fn [cause]
                       (def values
                         (mapcat (fn [e] (get e :values))
                                 (filter (fn [e] (= (get e :cause) cause)) mine)))
                       [cause
                        (passed (get fun :facts) (set ;(map spelled values)))])
                     raised)))

  (defn callee-name [answer f]
    "The glean callee F as a hand call spells it: a native's name or a closure
  constant's name as a keyword."
    (cond
      (string? f) (keyword f)
      (and (array? f) (= (get f 0) :fn))
        (let [name (get (fn/signature (get (get answer :closures) (get f 1)))
                        :name)]
          (if (nil? name) f (keyword name)))
      true (spelled f)))

  (defn named [unit name]
    "The function of UNIT whose name, spelled, is NAME, or nil."
    (def hit (filter (fn [u] (= (spelled (get u :name)) name)) unit))
    (if (empty? hit) nil (first hit)))

  (defn lines-of [u]
    "The set of the source lines the instructions of the unit function U are at."
    (set ;(map (fn [i] (get (get i :at) 0))
               (filter (fn [i] (has? i :at))
                       (mapcat (fn [b] (get b :instrs)) (get u :blocks []))))))

  (defn grafted [fence fun answer unit]
    "The set of the results and arguments of each hand call whose callee no glean
  call names, and of the values glean makes on that callee's lines, with each
  value they pass into."
    (def facts (get fence :facts))
    (def called
      (set ;(map (fn [t] (callee-name answer (get t 1)))
                 (get (get fun :facts) :call []))))
    (def out @[])
    (def lines @[])
    (each [r f p] in (get facts :call [])
      (unless (contains? called f)
        (push out r)
        (each [y _ q] in (get facts :arg [])
          (when (= q p) (push out y)))
        (let [callee (named unit f)]
          (unless (nil? callee) (push lines (lines-of callee))))))
    (def graft-lines (fold union || lines))
    (def nodes (get (view (named unit (spelled (get fence :name)))) :nodes))
    (def made
      (map (fn [[v _]] (spelled v))
           (filter (fn [[_ p]]
                     (def at (get (get (get nodes p) :node) :at))
                     (and (not (nil? at)) (contains? graft-lines (get at 0))))
                   (get (get fun :facts) :alloc []))))
    (union (set ;out) (passed (get fun :facts) (set ;made))))

  (defn closure-results [fence fun]
    "The set of the hand allocs whose value is the result of a glean call to a
  callee that is no native."
    (def results
      (set ;(map (fn [t] (spelled (get t 0)))
                 (filter (fn [t] (not (string? (get t 1))))
                         (get (get fun :facts) :call [])))))
    (set ;(filter (fn [v] (contains? results v))
                  (map (fn [t] (get t 0)) (get (get fence :facts) :alloc [])))))

  (defn bindings [unit]
    "The set of the names a :slots or :captures entry of UNIT binds, spelled."
    (def out @[])
    (each fun in unit
      (each b in (concat (get fun :slots []) (get fun :captures []))
        (unless (or (nil? b) (= b :synthetic)) (push out (spelled b)))))
    (set ;out))

  (defn compound? [name]
    "Whether NAME is a glean name of a slot, a register or a constant."
    (and (array? name) (contains? |:slot :reg :const| (get name 0))))

  (defn claims [fence answer unit noise raw judged]
    "One claim per name of the RAW row of FENCE: its judgment, side and value, and
  the set of causes that claim it. ANSWER and NOISE are what gleaning UNIT gave,
  and JUDGED is the row under the judge that makes each late capture a store."
    (if (has? raw :error)
      []
      (let [fun (first (filter (fn [f] (= (get f :name) (get fence :name)))
                               (get answer :functions)))
            by-noise (raised-names fun noise)
            graft (grafted fence fun answer unit)
            made-by-call (closure-results fence fun)
            holds (held (get fun :facts))
            bound (bindings unit)
            all (entries raw)]
        (defn unnamed-hand? [s n]
          (and (= s :hand) (not (contains? holds n)) (not (contains? bound n))))
        (defn unnamed? [j s n]
          (or (unnamed-hand? s n)
              (and (= s :glean) (compound? n)
                   (any? (fn [[j2 s2 n2]] (and (= j2 j) (unnamed-hand? s2 n2)))
                         all))))
        (defn captured? [j n]
          (and (not (has? judged :error))
               (not (contains? (set ;(get (get judged j) :hand)) n))
               (not (contains? (set ;(get (get judged j) :glean)) n))))
        (->array (map (fn [[j s n]]
                        (def by @[])
                        (each cause in raised
                          (when (contains? (get by-noise cause) n)
                            (push by cause)))
                        (when (contains? graft n) (push by :grafted))
                        (when (unnamed? j s n) (push by :unnamed))
                        (when (captured? j n) (push by :captures))
                        (when (and (= s :hand) (contains? made-by-call n))
                          (push by :closure-result))
                        {:document (get raw :document)
                         :name (get raw :name)
                         :judgment j
                         :side s
                         :value n
                         :causes (set ;by)}) all)))))

  (defn tally [claims]
    "Per cause, the names it claims and those it alone claims; the names more than
  one cause claims; and the claims no cause makes."
    (def claimed @{})
    (def alone @{})
    (each c in causes
      (put claimed c 0)
      (put alone c 0))
    (var shared 0)
    (def unclaimed @[])
    (each cl in claims
      (def by (get cl :causes))
      (when (empty? by) (push unclaimed cl))
      (when (> (length by) 1) (assign shared (+ shared 1)))
      (each c in by
        (put claimed c (+ 1 (get claimed c)))
        (when (= (length by) 1)
          (put alone c (+ 1 (get alone c))))))
    {:claimed (freeze claimed)
     :alone (freeze alone)
     :shared shared
     :unclaimed (freeze unclaimed)})

  {:causes causes :claims claims :tally tally})
