(elle/epoch 12)
## audited: 2026-09-21
## lib/semver/migrate.lisp — read a module's shipped migration rules and
## judge a major claim's coverage.
##
## docs/semver.md owns the coverage gate, docs/versioning.md the rule
## vocabulary; tests/elle/semver-check.lisp pins coverage end to end.
##
## Usage:
##   (def mig ((import "std/semver/migrate")))
##   (mig:rules source 2)            => [{:kind :rename ...} ...] | nil
##   (mig:uncovered rules changes)   => major changes no rule names
##   (mig:skeleton path major old new uncov) => the hint text

(fn []
  (defn fail [msg]
    (error {:error :migrate-error :message (string "migrate: " msg)}))

  (defn rule->record [r]
    "One rule form into {:kind ...}; the reader validated the shape."
    (let [a (->array r)
          head (string (a 0))]
      (cond
        (= head "rename")
          {:kind :rename :from (string (a 1)) :to (string (a 2))}
        (= head "replace")
          {:kind :replace
           :name (string ((->array (a 1)) 0))
           :pattern (a 1)
           :template (a 2)}
        (= head "remove")
          {:kind :remove :name (string (a 1)) :message (a 2)}
        (= head "warn")
          {:kind :warn :name (string (a 1)) :message (a 2)}
        (fail (string "unknown rule (" head " ...)")))))

  (defn rules [source major]
    "The records of (elle/migration MAJOR ...) in SOURCE, nil when the
     form is absent. A leading summary string is skipped."
    (block :found
      (each f (read-all source)
        (when (list? f)
          (let [a (->array f)]
            (when (and (>= (length a) 2) (= (string (a 0)) "elle/migration")
                       (= (a 1) major))
              (let [body (->list (slice a 2 (length a)))
                    body (if (and (not (empty? body)) (string? (first body)))
                           (rest body)
                           body)]
                (break :found (->array (map rule->record body))))))))
      nil))

  (defn rule-covers [r]
    (or (get r :from) (get r :name)))

  (defn covers? [recs name]
    (any? (fn [r] (= name (rule-covers r))) (->list (or recs []))))

  (defn uncovered [recs changes]
    "The major-floor changes no rule names; nil RECS cover nothing."
    (->list (filter (fn [c]
                      (and (= (c :floor) :major)
                           (not (covers? recs (string (c :export))))))
                    (->list changes))))

  ## ── the hint skeleton ────────────────────────────────────────────

  (defn shape-key [rec]
    (string (rec :required) "/" (rec :optional) "/" (string (rec :rest)) "/"
            (string/join (map (fn [k] (string k)) (->list (rec :named-keys)))
                         ",")))

  (defn added-by-shape [old new]
    "Exports NEW gained, keyed by their shape."
    (let [@out {}]
      (each k (keys (new :exports))
        (let [rec ((new :exports) k)]
          (when (and (not (get (old :exports) k)) (= (rec :kind) :fn))
            (assign out (put out (shape-key rec) k)))))
      out))

  (defn distinct-exports [changes]
    (let [@seen {}
          @out @[]]
      (each c changes
        (let [k (c :export)]
          (unless (get seen k)
            (assign seen (put seen k true))
            (push out c))))
      (->list out)))

  (defn suggest [old new c]
    "One rule suggestion for an uncovered change."
    (let [name (string (c :export))]
      (if (= (c :change) :export-removed)
        (let [rec (c :was)
              match (and (= (rec :kind) :fn)
                         (get (added-by-shape old new) (shape-key rec)))]
          (if match
            (string "(rename " name " " (string match) ")")
            (string "(remove " name " \"say what replaces it\")")))
        (string "(warn " name " \"describe the break\")"))))

  (defn skeleton [path major old new uncov]
    "The hint block: the migration form the module must ship."
    (let [cs (distinct-exports uncov)
          lines (map (fn [c] (string "    " (suggest old new c))) (->list cs))]
      (string "hint: " (length (->list cs))
              " major break(s) have no migration rule; add to " path ":\n"
              "  (elle/migration " major "\n" (string/join (->list lines) "\n")
              ")")))

  {:rules rules :uncovered uncovered :skeleton skeleton})
