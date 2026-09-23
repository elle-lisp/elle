(elle/epoch 12)
## audited: 2026-09-21
## lib/semver/migrate.lisp — a module's shipped migration rules: read
## them, judge a major claim's coverage, and repair consumer sources.
##
## docs/semver.md owns the coverage gate and the migrate command,
## docs/versioning.md the rule vocabulary; tests/elle/semver-check.lisp
## and tests/elle/semver-migrate.lisp pin both ends.
##
## Usage:
##   (def mig ((import "std/semver/migrate")))
##   (mig:rules source 2)            => [{:kind :rename ...} ...] | nil
##   (mig:uncovered rules changes)   => major changes no rule names
##   (mig:skeleton path major old new uncov) => the hint text
##   (mig:migrate-source {:source :specs :lib-source :exports :from})
##     => {:source :count :reports :manual}

(fn []
  (defn fail [msg]
    (error {:error :migrate-error :message (string "migrate: " msg)}))

  (defn has? [xs x]
    (any? (fn [y] (= y x)) (->list xs)))

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

  ## ── repairing consumers ──────────────────────────────────────────

  (defn majors [source]
    "The majors SOURCE ships migration forms for, ascending."
    (let [@out @[]]
      (each f (read-all source)
        (when (list? f)
          (let [a (->array f)]
            (when (and (>= (length a) 2) (= (string (a 0)) "elle/migration")
                       (integer? (a 1)))
              (push out (a 1))))))
      (->list (sort (->list out)))))

  (defn render-form [f qualify]
    "A rule form back to source text; QUALIFY maps a symbol's name."
    (cond
      (list? f)
        (string "("
                (string/join (map (fn [x] (render-form x qualify)) (->list f))
                             " ") ")")
      (array? f)
        (string "["
                (string/join (map (fn [x] (render-form x qualify)) (->list f))
                             " ") "]")
      (keyword? f) (string ":" (string f))
      (string? f) (string "\"" f "\"")
      (symbol? f) (qualify (string f))
      (string f)))

  (defn instantiate [records binding exports]
    "One binding's rules as compile/apply-rules structs. Template
     symbols naming an export re-qualify with the binding."
    (let [q (fn [name] (string binding ":" name))
          qual (fn [name] (if (has? exports name) (q name) name))]
      (->array (map (fn [r]
                      (match (r :kind)
                        :rename
                          {:kind :rename :from (q (r :from)) :to (q (r :to))}
                        :replace
                          {:kind :replace
                           :name (q (r :name))
                           :arity (- (length (->array (r :pattern))) 1)
                           :template (render-form (r :template) qual)}
                        _
                          {:kind :report
                           :name (q (r :name))
                           :message (r :message)})) (->list records)))))

  (defn removed-names [records binding]
    "The qualified names REMOVE rules take away from BINDING."
    (let [q (fn [name] (string binding ":" name))]
      (->list (map (fn [r] (q (r :name)))
                   (->list (filter (fn [r] (= (r :kind) :remove))
                                   (->list records)))))))

  (defn import-of [f specs]
    "The spec F imports, when F is (import \"spec\") for one of SPECS."
    (and (list? f)
         (let [a (->array f)]
           (and (= (length a) 2) (= (string (a 0)) "import") (string? (a 1))
                (has? specs (a 1)) (a 1)))))

  (defn binding-of [form specs]
    "NAME when FORM is (def name ((import spec) ...)); nil otherwise."
    (block :no
      (unless (list? form) (break :no nil))
      (let [a (->array form)]
        (unless (and (= (length a) 3) (= (string (a 0)) "def") (symbol? (a 1)))
          (break :no nil))
        (let [v (a 2)]
          (unless (list? v) (break :no nil))
          (let [va (->array v)]
            (unless (and (>= (length va) 1) (import-of (va 0) specs))
              (break :no nil))
            (string (a 1)))))))

  (defn count-imports [form specs]
    (let [@n 0]
      (defn walk [f]
        (when (import-of f specs) (assign n (inc n)))
        (when (or (list? f) (array? f))
          (each c f
            (walk c))))
      (walk form)
      n))

  (defn scan-consumer [source specs]
    "{:bindings :manual}: the def-bound module bindings, and how many
     imports of SPECS the tool cannot see through."
    (let [@bindings @[]
          @total 0
          @recognized 0]
      (each form (read-all source)
        (assign total (+ total (count-imports form specs)))
        (let [b (binding-of form specs)]
          (when b
            (assign recognized (inc recognized))
            (push bindings b))))
      {:bindings (->list bindings) :manual (- total recognized)}))

  (defn migrate-source [opts]
    "Apply the library's rules to one consumer source, majors ascending
     so renames chain. Reports carry :kind :remove or :warn."
    (let [specs (opts :specs)
          scan (scan-consumer (opts :source) specs)
          from (or (get opts :from) 0)
          @text (opts :source)
          @count 0
          @reports @[]]
      (each m (majors (opts :lib-source))
        (when (> m from)
          (let [records (rules (opts :lib-source) m)]
            (when records
              (each b (scan :bindings)
                (let [removed (removed-names records b)
                      r (compile/apply-rules text
                      (instantiate records b (opts :exports)))]
                  (assign text (r :source))
                  (assign count (+ count (r :count)))
                  (each rep (r :reports)
                    (push reports
                          (merge rep
                                 {:kind (if (has? removed (rep :name))
                                          :remove
                                          :warn)})))))))))
      {:source text
       :count count
       :reports (->list reports)
       :manual (scan :manual)}))

  {:rules rules
   :uncovered uncovered
   :skeleton skeleton
   :migrate-source migrate-source})
