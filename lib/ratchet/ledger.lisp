(elle/epoch 13)
# audited: 2026-09-30
## lib/ratchet/ledger.lisp — the committed bounds: a row per (subject, axis),
## the judge that meets a reading with its row, and the reading line.
## docs/ratchet.md
##
## Loaded by lib/ratchet.lisp and by the runner: the one judge every verdict
## comes from, in a direct run and under `elle test` alike.

(fn [& opts]
  # ── rows ──────────────────────────────────────────────────────────
  (defn row-key [subject axis]
    "The key a row and a reading meet on."
    (string subject "\t" (string axis)))

  (defn read-options [form from row]
    "Fold the keyword-value pairs of FORM from index FROM into ROW."
    (def @i from)
    (while (< (+ i 1) (length form))
      (let [k (get form i)
            v (get form (+ i 1))]
        (when (keyword? k) (put row k v)))
      (assign i (+ i 2)))
    row)

  (defn parse-row [form]
    "A ledger row, or nil when FORM is not one. A row is
     [subject axis bound & options]: the bound is a number (a pin), or :floor
     or :ceiling followed by one. Options default to a control with no slack,
     whose better side is lower."
    (if (not (and (array? form) (>= (length form) 3) (string? (get form 0))
                  (keyword? (get form 1))))
      nil
      (let [third (get form 2)
            row @{:subject (get form 0)
                  :axis (get form 1)
                  :class :control
                  :root nil
                  :better :lower
                  :slack 0
                  :note nil}]
        (cond
          (number? third) (begin
                            (put row :kind :pin)
                            (put row :bound third)
                            (read-options form 3 row))
          (and (or (= third :floor) (= third :ceiling)) (>= (length form) 4)
               (number? (get form 3)))
            (begin
              (put row :kind third)
              (put row :bound (get form 3))
              (read-options form 4 row))
          nil))))

  (defn header-of [form]
    "The producer a `(producer \"path\")` form names, or nil."
    (if (and (list? form) (= (length form) 2) (= (first form) (quote producer))
             (string? (get form 1)))
      (get form 1)
      nil))

  (defn load-file [path]
    "One ledger file: {:file :producer :rows}, or nil when no form in it is
     the `(producer \"path\")` header. Rows are keyed by `row-key`."
    (let [forms (read-all (slurp path))
          @producer nil
          @rows @{}]
      (each f in forms
        (let [p (header-of f)]
          (if p
            (assign producer p)
            (let [row (parse-row f)]
              (when row
                (put rows (row-key (get row :subject) (get row :axis)) row))))))
      (if producer {:file path :producer producer :rows rows} nil)))

  (defn load-dir [dir]
    "Every ledger under DIR, keyed by producer."
    (let [@out @{}]
      (each name in (file/ls dir)
        (when (string/ends-with? name ".lisp")
          (let [l (load-file (path/join dir name))]
            (when l (put out (get l :producer) l)))))
      out))

  (defn ledger-dir [root]
    "Where the ledgers are: ELLE_LEDGER, else tests/ledger under ROOT."
    (let [env (sys/env "ELLE_LEDGER")]
      (if (and env (> (length env) 0))
        env
        (if root (path/join root "tests/ledger") nil))))

  (defn producer-of [argv root]
    "The producer a program is: the path it was started with, relative to ROOT
     when it sits under ROOT. Nil when argv names no file — a form in a worker
     thread, or an -e snippet."
    (if (empty? argv)
      nil
      (let [a (first argv)]
        (if (and (string? a) (file/exists? a))
          (if (and root (path/absolute? a)
                   (string/starts-with? a (string root "/")))
            (path/relative a root)
            a)
          nil))))

  # ── the judge ─────────────────────────────────────────────────────
  (defn judge [reading row]
    "The verdict of READING against ROW: :ok, :regression, :stale, :void, or
     :unledgered when ROW is nil. A pin is two-sided and a floor or a ceiling
     one-sided; every comparison is on the reading's whole interval."
    (cond
      (get reading :void) :void
      (nil? row) :unledgered
      true
        (let [v (get reading :value)
              h (or (get reading :half) 0)
              lo (- v h)
              hi (+ v h)
              bound (get row :bound)
              s (or (get row :slack) 0)
              kind (get row :kind)]
          (case kind
            :floor
              (if (< hi bound)
                (if (= (get row :class) :growth) :void :regression)
                :ok)
            :ceiling (if (> lo bound) :regression :ok)
            (if (= (get row :better) :higher)
              (cond
                (< hi (- bound s)) :regression
                (> lo (+ bound s)) :stale
                true :ok)
              (cond
                (> lo (+ bound s)) :regression
                (< hi (- bound s)) :stale
                true :ok))))))

  (defn judge-with [rows void-axes reading]
    "READING judged against ROWS, carrying its row's bound and kind. A growth
     row that read flat marks its axis in VOID-AXES, and every other reading
     on a marked axis is void with the reason."
    (let [row (get rows (row-key (get reading :subject) (get reading :axis)))
          verdict (judge reading row)
          axis (get reading :axis)
          growth? (or (= (get reading :class) :growth)
                      (and row (= (get row :class) :growth)))
          r0 (if row
               (put (put reading :bound (get row :bound)) :kind (get row :kind))
               reading)]
      (when (and growth? (= verdict :void) (not (get reading :void)))
        (put void-axes axis
             (string (get reading :subject) " read " (get reading :value) " "
                     (get reading :unit) " — the gauge is dead")))
      (cond
        (and (not growth?) (get void-axes axis))
          (put (put r0 :verdict :void)
               :why (string "axis void: " (get void-axes axis)))
        (= verdict :void)
          (put (put r0 :verdict :void)
               :why (or (get reading :void) (get void-axes axis)))
        true (put r0 :verdict verdict))))

  (defn judge-all [readings rows]
    "Every reading in order, through `judge-with`, with one void-axis set."
    (let [@void-axes @{}]
      (map (fn [r] (judge-with rows void-axes r)) readings)))

  (defn unread [rows readings]
    "Every row of ROWS that no reading in READINGS answers."
    (let [@seen @{}
          @out @[]]
      (each r in readings
        (put seen (row-key (get r :subject) (get r :axis)) true))
      (each k in (keys rows)
        (when (not (get seen k)) (push out (get rows k))))
      out))

  (defn plain [n]
    "A number as a row writes it: an integral float as the integer it is. A
     bound read back from a REAL column arrives as a float either way."
    (if (and (float? n) (= n (float (int n)))) (string (int n)) (string n)))

  (defn describe-bound [kind bound]
    "The bound a reading met, for a reader: `pinned 42`, `floor 0.5`,
     `ceiling 8`, or `no row`."
    (case kind
      :pin (string "pinned " (plain bound))
      :floor (string "floor " (plain bound))
      :ceiling (string "ceiling " (plain bound))
      "no row"))

  # ── the line ──────────────────────────────────────────────────────
  (def marker "measure ")

  (defn line-fields [r]
    "The reading's fields as the line carries them: keywords become names."
    (let [@out @{:subject (get r :subject)
                 :axis (string (get r :axis))
                 :value (get r :value)
                 :half (or (get r :half) 0)
                 :unit (get r :unit)}]
      (each k in [:bound :why :floor :void :alt-value]
        (when (not (nil? (get r k))) (put out k (get r k))))
      (each k in [:kind :verdict :class]
        (when (not (nil? (get r k)))
          (put out k (string (get r k)))))
      out))

  (defn render-reading [r]
    "One reading as its line."
    (string marker (json/serialize (line-fields r))))

  (defn reading-of-line [line]
    "The reading a line carries, or nil when the line is not one."
    (if (not (string/starts-with? line marker))
      nil
      (let [[ok? rec] (protect (json/parse (slice line (length marker)
                               (length line)) :keys :keyword))]
        (if (not (and ok? (struct? rec) (string? (get rec :subject))
                      (string? (get rec :axis))))
          nil
          (let [@r @{}]
            (each k in (keys rec)
              (put r k (get rec k)))
            (put r :axis (keyword (get rec :axis)))
            (when (nil? (get r :half)) (put r :half 0))
            (each k in [:kind :verdict :class]
              (when (string? (get r k))
                (put r k (keyword (get r k)))))
            r)))))

  (defn readings-in [text]
    "Every reading printed in TEXT, one per `measure` line, in order."
    (let [@out @[]]
      (each line in (string/split text "\n")
        (let [r (reading-of-line line)]
          (when r (push out r))))
      out))

  {:row-key row-key
   :parse-row parse-row
   :load-file load-file
   :load-dir load-dir
   :ledger-dir ledger-dir
   :producer-of producer-of
   :judge judge
   :judge-with judge-with
   :judge-all judge-all
   :unread unread
   :plain plain
   :describe-bound describe-bound
   :render-reading render-reading
   :readings-in readings-in})
