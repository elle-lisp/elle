(elle/epoch 12)
## audited: 2026-09-21
## lib/semver/surface.lisp — extract a module's public surface
##
## Hybrid extraction: the module is analyzed AND loaded. Runtime
## reflection is the authority on counts, keys, signals and traits — it
## sees through re-exports; static analysis supplies parameter names and
## is the whole answer when construction signals (mode :static).
## docs/versioning.md owns the record vocabulary;
## tests/elle/semver-surface.lisp pins this module.
##
## Usage:
##   (def surf ((import "std/semver/surface")))
##   (surf:extract {:path "lib/x.lisp" :module "std/x"}) => surface

(fn []
  (defn fail [msg]
    (error {:error :surface-error :message (string "surface: " msg)}))

  (defn freeze [xs]
    "An immutable array of XS, whatever sequence it arrives as."
    (->array (->list xs)))

  (defn pad8 [s]
    (if (< (length s) 8) (pad8 (string "0" s)) s))

  (defn short-hash [s]
    "An 8-hex rolling hash of a string. Pure Elle, so the answer never
     moves with the build that computes it."
    (let [bs (bytes s)
          @h 5381
          @i 0]
      (while (< i (length bs))
        (assign h (mod (+ (* h 33) (get bs i)) 4294967291))
        (assign i (inc i)))
      (pad8 (seq->hex h))))

  (defn kws->array [s]
    "A set or sequence of keywords as a name-sorted immutable array."
    (freeze (sort-by string (->list s))))

  (defn ints->array [s]
    (freeze (sort (->list s))))

  (defn signals-record [sig]
    {:bits (kws->array (sig :bits)) :propagates (ints->array (sig :propagates))})

  (defn with-doc [rec doc]
    (if doc
      (merge rec {:doc (short-hash doc)})
      rec))

  ## ── records from runtime reflection ─────────────────────────────

  (defn rt-fn-record [f static-rec]
    (let [sig (fn/signature f)]
      (with-doc {:kind :fn
                 :required (sig :required)
                 :optional (sig :optional)
                 :rest (sig :rest)
                 :named-keys (sig :named-keys)
                 :params (or (and static-rec (get static-rec :params)) [])
                 :signals (signals-record (sig :signals))} (get sig :doc))))

  (defn default-traits-of [v]
    "The builtin trait table a fresh value of V's type carries."
    (match (type-of v)
      :array (traits [0])
      :@array (traits @[0])
      :list (traits (list 0))
      :string (traits "probe")
      :@string (traits @"probe")
      :struct (traits {:probe 0})
      :@struct (traits @{:probe 0})
      :bytes (traits (bytes 0))
      :set (traits |0|)
      _ nil))

  (defn traits-record [table]
    "Protocol → sorted method names; operator methods collect as :ops."
    (let [@out {}
          @ops @[]]
      (each k (sort-by string (->list (keys table)))
        (let [entry (table k)]
          (if (struct? entry)
            (assign out (put out k (kws->array (keys entry))))
            (push ops k))))
      (if (empty? (->list ops)) out (put out :ops (kws->array ops)))))

  (defn rt-value-record [v]
    (let [base {:kind :value :type (type-of v) :hash (short-hash (string v))}
          table (traits v)]
      (if (and table (not (identical? table (default-traits-of v))))
        (merge base {:traits (traits-record table)})
        base)))

  ## ── records from static analysis ────────────────────────────────

  (defn static-fn-record [rec]
    (with-doc {:kind :fn
               :required (rec :required)
               :optional (rec :optional)
               :rest (rec :rest)
               :named-keys (rec :named-keys)
               :params (rec :params)
               :signals (signals-record (rec :signals))} (get rec :doc)))

  (defn static-record [rec]
    (if (= (rec :kind) :fn)
      (static-fn-record rec)
      # A value's type and contents exist only at run time.
      {:kind :value :type :unknown :hash "00000000"}))

  (defn shape-only [rec params]
    {:required (rec :required)
     :optional (rec :optional)
     :rest (rec :rest)
     :named-keys (rec :named-keys)
     :params (or params [])})

  ## ── the two extraction modes ────────────────────────────────────

  (defn declared-version [source]
    (block :found
      (each f (read-all source)
        (when (list? f)
          (let [a (->array f)]
            (when (and (= (length a) 2) (= (string (a 0)) "elle/version")
                       (string? (a 1)))
              (break :found (a 1))))))
      nil))

  (defn static-surface [source path]
    "The analyzed surface: {:constructor <rec|nil> :exports {..}}, or
     nil when the file returns no export struct."
    (let [[ok? analysis] (protect (compile/analyze source {:file path}))]
      (if ok? (compile/exports analysis) nil)))

  (defn nils [n]
    (let [@l ()
          @i 0]
      (while (< i n)
        (assign l (pair nil l))
        (assign i (inc i)))
      l))

  (defn hybrid-exports [inst sx]
    (let [@out {}]
      (each k (keys inst)
        (let [v (inst k)
              static-rec (and sx (get (sx :exports) k))]
          (assign
            out
            (put out k
                 (if (fn? v) (rt-fn-record v static-rec) (rt-value-record v))))))
      out))

  (defn static-exports [sx]
    (let [@out {}]
      (each k (keys (sx :exports))
        (assign out (put out k (static-record ((sx :exports) k)))))
      out))

  (defn assemble [module version mode constructor exports]
    (let [@s {:format 1 :module module :mode mode :exports exports}]
      (when version (assign s (put s :version version)))
      (when constructor (assign s (put s :constructor constructor)))
      s))

  (defn static-mode [module version sx]
    (unless sx (fail (string module ": no export struct to analyze")))
    (assemble module version
              :static (and (get sx :constructor)
                           (shape-only (sx :constructor)
                                       (get (sx :constructor) :params)))
              (static-exports sx)))

  (defn extract [opts]
    "The surface of the module at (opts :path), named (opts :module)."
    (let [path (opts :path)
          module (opts :module)
          source (file/read path)
          version (declared-version source)
          sx (static-surface source path)
          ctor (import-file path)]
      (cond
        (fn? ctor)
          (let [csig (fn/signature ctor)
                shape (shape-only csig
                                  (and sx (get sx :constructor)
                                       ((sx :constructor) :params)))
                [ok? inst] (protect (apply ctor (nils (csig :required))))]
            (if (and ok? (struct? inst))
              (assemble module version :hybrid shape (hybrid-exports inst sx))
              (static-mode module version sx)))
        (struct? ctor) (assemble module version :hybrid nil
                                 (hybrid-exports ctor sx))
        (static-mode module version sx))))

  {:extract extract :short-hash short-hash})
