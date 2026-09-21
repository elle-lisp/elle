(elle/epoch 12)
# audited: 2026-09-21
# std/semver/diff — the floor table of docs/versioning.md, one assertion
# per row, plus the claim verdict.
#
# The counter-factual: the floor is the tool's whole authority. A row
# that drifts from the table turns a breaking release into a quiet minor,
# and nothing downstream would catch it.

(def sdiff ((import "std/semver/diff")))

(defn fnrec [over]
  (merge {:kind :fn
          :required 1
          :optional 0
          :rest :none
          :named-keys []
          :params ["a"]
          :signals {:bits [] :propagates []}} over))

(defn surface [exports]
  {:format 1 :module "m" :version "1.0.0" :mode :hybrid :exports exports})

(defn floor-of [old-rec new-rec]
  ((sdiff:diff (surface {:f old-rec}) (surface {:f new-rec})) :floor))

(def base (fnrec {}))
(def valrec {:kind :value :type :integer :hash "00000001"})

# ── the floor table ────────────────────────────────────────────────
(assert (= ((sdiff:diff (surface {:f base}) (surface {})) :floor) :major)
        "export removed")
(assert (= ((sdiff:diff (surface {}) (surface {:f base})) :floor) :minor)
        "export added")
(assert (= (floor-of base valrec) :major) "kind flip fn to value")
(assert (= (floor-of base (fnrec {:required 2 :params ["a" "b"]})) :major)
        "required arity increased")
(assert (= (floor-of (fnrec {:required 2 :params ["a" "b"]})
                     (fnrec {:required 1 :optional 1 :params ["a" "b"]})) :minor)
        "required parameter became optional")
(assert (= (floor-of base (fnrec {:optional 1 :params ["a" "b"]})) :minor)
        "new optional parameter")
(assert (= (floor-of (fnrec {:optional 1 :params ["a" "b"]}) base) :major)
        "maximum arity decreased")
(assert (= (floor-of base (fnrec {:rest :list})) :minor)
        "new rest collector widens")
(assert (= (floor-of (fnrec {:rest :list}) base) :major)
        "rest collector removed")
(assert (= (floor-of (fnrec {:rest :named :named-keys [:a]})
                     (fnrec {:rest :keys})) :minor)
        "&named to &keys is strict to open")
(assert (= (floor-of (fnrec {:rest :keys})
                     (fnrec {:rest :named :named-keys [:a]})) :major)
        "&keys to &named narrows")
(assert (= (floor-of (fnrec {:rest :named :named-keys [:a :b]})
                     (fnrec {:rest :named :named-keys [:a]})) :major)
        "&named key removed")
(assert (= (floor-of (fnrec {:rest :named :named-keys [:a]})
                     (fnrec {:rest :named :named-keys [:a :b]})) :minor)
        "&named key added")
(assert (= (floor-of base (fnrec {:params ["renamed"]})) :none)
        "positional rename moves nothing")
(assert (= (floor-of base (fnrec {:signals {:bits [:error] :propagates []}}))
           :major) "signal bit added")
(assert (= (floor-of (fnrec {:signals {:bits [:error] :propagates []}}) base)
           :patch) "signal bit removed")
(assert (= (floor-of base (fnrec {:signals {:bits [] :propagates [0]}})) :major)
        "propagates index added")
(assert (= (floor-of (fnrec {:signals {:bits [] :propagates [0]}}) base) :patch)
        "propagates index removed")
(assert (= (floor-of base (fnrec {:doc "12ab34cd"})) :patch) "docstring changed")
(assert (= (floor-of valrec {:kind :value :type :string :hash "00000001"})
           :major) "value type changed")
(assert (= (floor-of valrec {:kind :value :type :integer :hash "0000ffff"})
           :patch) "value hash changed")
(assert (= (floor-of (merge valrec {:traits {:Collection [:conj :empty]}})
                     (merge valrec {:traits {:Collection [:conj]}})) :major)
        "trait method removed")
(assert (= (floor-of (merge valrec {:traits {:Collection [:conj]}})
                     (merge valrec {:traits {:Collection [:conj :empty]}}))
           :minor) "trait method added")
(assert (= (floor-of base base) :none) "identical surfaces move nothing")

# The constructor obeys the same rules.
(let [old (merge (surface {:f base})
                 {:constructor {:required 1
                                :optional 0
                                :rest :none
                                :named-keys []
                                :params ["dep"]}})
      new (merge (surface {:f base})
                 {:constructor {:required 2
                                :optional 0
                                :rest :none
                                :named-keys []
                                :params ["dep" "cfg"]}})]
  (assert (= ((sdiff:diff old new) :floor) :major)
          "constructor shape change is a fn change"))

# Changes carry the export they name and their own floor.
(let [d (sdiff:diff (surface {:f base :gone base})
                    (surface {:f (fnrec {:doc "12ab34cd"})}))]
  (assert (= (d :floor) :major) "max over all changes")
  (assert (= (length (d :changes)) 2) "one change per finding")
  (assert (any? (fn [c] (and (= (c :export) :gone) (= (c :floor) :major)))
                (d :changes)) "the removal names its export"))

# ── the claim verdict ──────────────────────────────────────────────
(assert (= (sdiff:required "1.2.3" :major) "2.0.0") "major floor")
(assert (= (sdiff:required "1.2.3" :minor) "1.3.0") "minor floor")
(assert (= (sdiff:required "1.2.3" :patch) "1.2.4") "patch floor")
(assert (= (sdiff:required "1.2.3" :none) "1.2.3") "no change, no bump")
(assert (= (sdiff:required "0.3.1" :major) "0.4.0") "pre-1.0 major bumps y")
(assert (= (sdiff:required "0.3.1" :minor) "0.3.2") "pre-1.0 minor bumps z")

(let [v (sdiff:verdict {:baseline "1.2.3" :claimed "1.3.0" :floor :major})]
  (assert (= (v :verdict) :insufficient) "claim below the floor")
  (assert (= (v :required) "2.0.0") "the verdict names the requirement"))
(let [v (sdiff:verdict {:baseline "1.2.3" :claimed "2.0.0" :floor :major})]
  (assert (= (v :verdict) :sufficient) "claim meets the floor"))
(let [v (sdiff:verdict {:baseline "1.2.3" :claimed "1.2.3" :floor :patch})]
  (assert (= (v :verdict) :insufficient) "an unbumped patch claim"))
(let [v (sdiff:verdict {:baseline "1.2.3" :claimed "1.2.3" :floor :none})]
  (assert (= (v :verdict) :sufficient) "nothing changed, nothing owed"))

(println "semver-diff: all tests passed")
