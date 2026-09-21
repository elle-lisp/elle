(elle/epoch 12)
## audited: 2026-09-21
## lib/semver/diff.lisp — classify surface changes and verify a claim
##
## The floor table in docs/versioning.md is the authority; each change
## carries its own floor and the release floor is the maximum. The
## verdict compares a claimed version against the floor with std/semver.
## tests/elle/semver-diff.lisp pins every table row.
##
## Usage:
##   (def sdiff ((import "std/semver/diff")))
##   (sdiff:diff old-surface new-surface) => {:floor kw :changes [..]}
##   (sdiff:required "1.2.3" :major)      => "2.0.0"
##   (sdiff:verdict {:baseline b :claimed c :floor f})
##     => {:verdict :sufficient|:insufficient :required v}

(fn []
  (def sv ((import "std/semver")))

  (defn fail [msg]
    (error {:error :semver-diff-error :message (string "diff: " msg)}))

  (defn rank [floor]
    (match floor
      :none 0
      :patch 1
      :minor 2
      :major 3
      _ (fail (string "unknown floor " floor))))

  (defn change [export kind floor was now]
    {:export export :change kind :floor floor :was was :now now})

  (defn has? [xs x]
    (any? (fn [y] (= y x)) (->list xs)))

  (defn sorted-keys [s]
    (sort-by string (->list (keys s))))

  (defn union-keys [a b]
    "Keys of both structs, each once, ordered by name."
    (let [@seen {}]
      (each k (keys a)
        (assign seen (put seen k true)))
      (each k (keys b)
        (assign seen (put seen k true)))
      (sorted-keys seen)))

  (defn each-only [out name kind floor from to]
    "One change per element of FROM that TO lacks."
    (each x from
      (unless (has? to x) (push out (change name kind floor x nil)))))

  ## ── one export, one record kind at a time ───────────────────────

  (defn shape= [old new]
    (and (= (old :required) (new :required)) (= (old :optional) (new :optional))
         (= (old :rest) (new :rest)) (= (old :named-keys) (new :named-keys))))

  (defn diff-shape [out name old new]
    "The parameter-shape rows of the floor table, for fns and the
     constructor alike."
    (when (> (new :required) (old :required))
      (push out
            (change name :required-arity-increased :major (old :required)
                    (new :required))))
    (when (< (new :required) (old :required))
      (push out
            (change name :required-became-optional :minor (old :required)
                    (new :required))))
    (let [old-max (+ (old :required) (old :optional))
          new-max (+ (new :required) (new :optional))]
      (when (< new-max old-max)
        (push out (change name :max-arity-decreased :major old-max new-max)))
      (when (and (> new-max old-max) (> (new :optional) (old :optional)))
        (push out (change name :optional-added :minor old-max new-max))))
    (when (not= (old :rest) (new :rest))
      (cond
        (= (old :rest) :none)
          (push out (change name :rest-added :minor :none (new :rest)))
        (and (= (old :rest) :named) (= (new :rest) :keys)) (push out
        (change name :rest-opened :minor :named :keys))
        (push out (change name :rest-changed :major (old :rest) (new :rest)))))
    (when (and (= (old :rest) :named) (= (new :rest) :named))
      (each-only out name :named-key-removed :major (old :named-keys)
                 (new :named-keys))
      (each-only out name :named-key-added :minor (new :named-keys)
                 (old :named-keys)))
    (when (and (shape= old new) (not= (old :params) (new :params)))
      (push out (change name :param-renamed :none (old :params) (new :params)))))

  (defn diff-fn [out name old new]
    (diff-shape out name old new)
    (let [os (old :signals)
          ns (new :signals)]
      (each-only out name :signal-added :major (ns :bits) (os :bits))
      (each-only out name :signal-removed :patch (os :bits) (ns :bits))
      (each-only out name :propagates-added :major (ns :propagates)
                 (os :propagates))
      (each-only out name :propagates-removed :patch (os :propagates)
                 (ns :propagates)))
    (when (not= (get old :doc) (get new :doc))
      (push out (change name :doc-changed :patch (get old :doc) (get new :doc)))))

  (defn trait-pairs [traits]
    "A trait table flattened to [protocol method] pairs."
    (let [@out @[]]
      (each p (sorted-keys (or traits {}))
        (each m ((or traits {}) p)
          (push out [p m])))
      out))

  (defn diff-value [out name old new]
    (when (not= (old :type) (new :type))
      (push out (change name :type-changed :major (old :type) (new :type))))
    (when (not= (old :hash) (new :hash))
      (push out (change name :hash-changed :patch (old :hash) (new :hash))))
    (let [op (trait-pairs (get old :traits))
          np (trait-pairs (get new :traits))]
      (each-only out name :trait-removed :major op np)
      (each-only out name :trait-added :minor np op)))

  (defn diff-export [out name old new]
    (cond
      (not= (old :kind) (new :kind))
        (push out (change name :kind-flipped :major (old :kind) (new :kind)))
      (= (old :kind) :fn) (diff-fn out name old new)
      (diff-value out name old new)))

  ## ── whole surfaces ──────────────────────────────────────────────

  (defn diff-constructor [out old new]
    (let [oc (get old :constructor)
          nc (get new :constructor)]
      (cond
        (and oc nc) (diff-shape out :constructor oc nc)
        oc (push out (change :constructor :constructor-removed :major oc nil))
        nc (push out (change :constructor :constructor-added :major nil nc))
        nil)))

  (defn diff [old new]
    "Every change between two surfaces, each with its floor, and the
     release floor: their maximum, none < patch < minor < major."
    (let [@out @[]
          oe (old :exports)
          ne (new :exports)]
      (diff-constructor out old new)
      (each k (union-keys oe ne)
        (let [orec (get oe k)
              nrec (get ne k)]
          (cond
            (and orec nrec) (diff-export out k orec nrec)
            orec (push out (change k :export-removed :major orec nil))
            (push out (change k :export-added :minor nil nrec)))))
      (let [changes (->array (->list out))
            @floor :none]
        (each c changes
          (when (> (rank (c :floor)) (rank floor)) (assign floor (c :floor))))
        {:floor floor :changes changes})))

  ## ── the claim verdict ───────────────────────────────────────────

  (defn required [baseline floor]
    "The lowest version that satisfies FLOOR over BASELINE. Pre-1.0
     follows cargo: a major floor bumps y, minor and patch bump z."
    (let [v (sv:parse baseline)
          pre? (zero? v:major)]
      (match floor
        :none baseline
        :major (sv:increment baseline (if pre? :minor :major))
        :minor (sv:increment baseline (if pre? :patch :minor))
        :patch (sv:increment baseline :patch)
        _ (fail (string "unknown floor " floor)))))

  (defn verdict [claim]
    "Whether (claim :claimed) meets the floor over (claim :baseline)."
    (let [need (required (claim :baseline) (claim :floor))
          ok? (>= (sv:compare (claim :claimed) need) 0)]
      {:verdict (if ok? :sufficient :insufficient) :required need}))

  {:diff diff :required required :verdict verdict})
