(elle/epoch 12)
## audited: 2026-09-21
## lib/semver/file.lisp — read and write .surface files
##
## docs/versioning.md fixes the format; tests/elle/semver-file.lisp pins
## the bytes. Rendering sorts exports by name and keeps a fixed field
## order per line, so two equal surfaces always render byte-identically.
##
## Usage:
##   (def sfile ((import "std/semver/file")))
##   (sfile:render surface)   => text
##   (sfile:parse text)       => surface

(fn []
  (defn fail [msg]
    (error {:error :surface-error :message (string "surface: " msg)}))

  (defn q [s]
    (string "\"" s "\""))

  (defn sorted-names [xs]
    "Values ordered by their printed name."
    (->list (sort-by string xs)))

  (defn render-kws [ks]
    "Keywords as :a :b, space-separated, in the given order."
    (string/join (map (fn [k] (string ":" (string k))) ks) " "))

  (defn render-shape [rec]
    "The parameter vector: required, &opt block, then the collector."
    (let [params (or (get rec :params) [])
          required (rec :required)
          optional (rec :optional)
          name-at (fn [i] (if (< i (length params)) (params i) "_"))
          @parts @[]
          @i 0]
      (while (< i required)
        (push parts (name-at i))
        (assign i (inc i)))
      (when (> optional 0)
        (push parts "&opt")
        (while (< i (+ required optional))
          (push parts (name-at i))
          (assign i (inc i))))
      (match (rec :rest)
        :none nil
        :list (do
                (push parts "&")
                (push parts "rest"))
        :keys (do
                (push parts "&keys")
                (push parts "opts"))
        :named
          (do
            (push parts "&named")
            (each k (rec :named-keys)
              (push parts (string ":" (string k)))))
        _
          (fail (string "unknown rest kind " (rec :rest))))
      (string "[" (string/join (->list parts) " ") "]")))

  (defn render-traits [traits]
    "{:Proto [:m1 :m2] ...} with protocols ordered by name."
    (let [protos (sorted-names (keys traits))
          one (fn [p] (string ":" (string p) " [" (render-kws (traits p)) "]"))]
      (string "{" (string/join (map one protos) " ") "}")))

  (defn render-export [name rec]
    (match (rec :kind)
      :fn
        (string "(export " (string name) " :fn " (render-shape rec)
                " :signals [" (render-kws ((rec :signals) :bits)) "]"
                (let [props ((rec :signals) :propagates)]
                  (if (empty? props)
                    ""
                    (string " :propagates ["
                            (string/join (map string props) " ") "]")))
                (if (get rec :doc)
                  (string " :doc " (q (rec :doc)))
                  "") ")")
      :value
        (string "(export " (string name) " :value :type :" (string (rec :type))
                " :hash " (q (rec :hash))
                (if (get rec :traits)
                  (string " :traits " (render-traits (rec :traits)))
                  "") ")")
      _
        (fail (string "unknown export kind " (rec :kind)))))

  (defn render [surface]
    "The surface as .surface text, one form per line, exports sorted."
    (let [@lines @[]]
      (push lines "(elle-surface 1)")
      (push lines (string "(module " (q (surface :module)) ")"))
      (push lines (string "(version " (q (surface :version)) ")"))
      (push lines (string "(mode :" (string (surface :mode)) ")"))
      (when (get surface :released)
        (let [r (surface :released)]
          (push lines
                (string "(released :commit " (q (r :commit)) " :date "
                        (q (r :date)) ")"))))
      (when (get surface :tests)
        (push lines (string "(tests " (q (surface :tests)) ")")))
      (when (get surface :constructor)
        (push lines
              (string "(constructor " (render-shape (surface :constructor)) ")")))
      (each name (sorted-names (keys (surface :exports)))
        (push lines (render-export name ((surface :exports) name))))
      (string (string/join (->list lines) "\n") "\n")))

  ## ── parsing ──────────────────────────────────────────────────────

  (defn parse-shape [shape]
    "A parameter vector back into its shape record."
    (let [@mode :req
          @required 0
          @optional 0
          @rest-kind :none
          @named @[]
          @names @[]]
      (each it shape
        (let [s (string it)]
          (cond
            (= s "&opt") (assign mode :opt)
            (= s "&") (do
                        (assign mode :collector)
                        (assign rest-kind :list))
            (= s "&keys") (do
                            (assign mode :collector)
                            (assign rest-kind :keys))
            (= s "&named") (do
                             (assign mode :named)
                             (assign rest-kind :named))
            (keyword? it)
              (if (= mode :named)
                (push named it)
                (fail (string "keyword " s " outside &named")))
            (= mode :req)
              (do
                (push names s)
                (assign required (inc required)))
            (= mode :opt)
              (do
                (push names s)
                (assign optional (inc optional)))
            (= mode :collector) nil
            (fail (string "misplaced " s " in shape")))))
      {:required required
       :optional optional
       :rest rest-kind
       :named-keys (->array (->list named))
       :params (->array (->list names))}))

  (defn parse-opts [items start]
    "Alternating :key value pairs from START to the end of ITEMS."
    (let [@opts {}
          @i start]
      (while (< i (length items))
        (let [k (items i)]
          (unless (keyword? k)
            (fail (string "expected a keyword option, got " (string k))))
          (when (>= (inc i) (length items))
            (fail (string "option :" (string k) " has no value")))
          (assign opts (put opts k (items (inc i))))
          (assign i (+ i 2))))
      opts))

  (defn datum->struct [form]
    "A read {:k v} literal arrives as the (struct :k v ...) form; build
     the struct it denotes."
    (let [items (->array form)]
      (unless (and (> (length items) 0) (= (string (items 0)) "struct"))
        (fail "expected a struct literal"))
      (let [@out {}
            @i 1]
        (while (< i (length items))
          (assign out (put out (items i) (items (inc i))))
          (assign i (+ i 2)))
        out)))

  (defn parse-export [items]
    "(export name kind ...) into [keyword record]."
    (when (< (length items) 3) (fail "short export form"))
    (let [name (keyword (string (items 1)))
          kind (items 2)]
      (match kind
        :fn
          (let [shape (parse-shape (items 3))
                opts (parse-opts items 4)
                sigs (get opts :signals)]
            (unless sigs
              (fail (string "export " (string name) " carries no :signals")))
            [name
             (merge shape
                    (merge {:kind :fn
                            :signals {:bits sigs
                                      :propagates (or (get opts :propagates) [])}}
                           (if (get opts :doc) {:doc (opts :doc)} {})))])
        :value
          (let [opts (parse-opts items 3)]
            (unless (and (get opts :type) (get opts :hash))
              (fail (string "export " (string name) " needs :type and :hash")))
            [name
             (merge {:kind :value :type (opts :type) :hash (opts :hash)}
                    (if (get opts :traits)
                      {:traits (datum->struct (opts :traits))}
                      {}))])
        _
          (fail (string "unknown export kind " (string kind))))))

  (defn parse [text]
    "A .surface text back into its surface struct."
    (let [forms (read-all text)]
      (when (empty? forms) (fail "empty file"))
      (let [head (->array (first forms))]
        (unless (and (= (length head) 2) (= (string (head 0)) "elle-surface")
                     (= (head 1) 1))
          (fail "not an (elle-surface 1) file")))
      (let [@surface {:format 1}
            @exports {}]
        (each form (rest forms)
          (let [items (->array form)
                head (string (items 0))]
            (cond
              (= head "module")
                (assign surface (put surface :module (items 1)))
              (= head "version")
                (assign surface (put surface :version (items 1)))
              (= head "mode")
                (assign surface (put surface :mode (items 1)))
              (= head "tests")
                (assign surface (put surface :tests (items 1)))
              (= head "released")
                (let [opts (parse-opts items 1)]
                  (assign
                    surface
                    (put surface
                         :released {:commit (opts :commit) :date (opts :date)})))
              (= head "constructor")
                (assign
                  surface
                  (put surface :constructor (parse-shape (items 1))))
              (= head "export")
                (let [[name rec] (parse-export items)]
                  (assign exports (put exports name rec)))
              (fail (string "unknown form (" head " ...)")))))
        (each field [:module :version :mode]
          (unless (get surface field)
            (fail (string "missing (" (string field) " ...)"))))
        (put surface :exports exports))))

  {:render render :parse parse})
