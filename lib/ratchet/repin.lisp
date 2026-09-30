(elle/epoch 13)
# audited: 2026-09-30
## lib/ratchet/repin.lisp — moving a ledger to what a run read: one bound token
## replaced inside its row's brackets, and one row appended per adopted reading.
## docs/ratchet.md
##
## Loaded by the runner for `elle test --repin`. The rewrite is over the file's
## text, so a comment above a row and the wrapping `elle fmt` gave it survive.

(fn [& opts]
  (def led ((import "std/ratchet/ledger")))

  # ── numbers ───────────────────────────────────────────────────────
  (defn floor-of [y]
    (let [t (math/trunc y)]
      (if (< y t) (- t 1.0) t)))

  (defn round-of [y]
    (if (< y 0.0)
      (- 0.0 (math/trunc (+ (- 0.0 y) 0.5)))
      (math/trunc (+ y 0.5))))

  (defn write-number [value half]
    "A bound as a row writes it: a count — an exact integral reading — as its
     integer, and a rate to three significant figures."
    (let [v (float value)]
      (cond
        (= v 0.0) "0"
        (and (= (float half) 0.0) (= v (float (int v)))) (string (int v))
        true
          (let [e (floor-of (math/log10 (abs v)))]
            (if (>= e 2.0)
              (let [scale (math/pow 10.0 (- e 2.0))]
                (led:plain (* (round-of (/ v scale)) scale)))
              (let [scale (math/pow 10.0 (- 2.0 e))]
                (led:plain (/ (round-of (* v scale)) scale))))))))

  (defn quote-string [s]
    "S as the string literal a row writes it as."
    (string "\"" (string/replace (string/replace s "\\" "\\\\") "\"" "\\\"")
            "\""))

  # ── the scanner ───────────────────────────────────────────────────
  # A ledger file holds string literals, keywords, numbers, brackets, the
  # header list, and comments to the end of a line. A row is one top-level
  # bracket form, and its tokens are what the rewrite reads and replaces.
  (defn string-end [text i]
    "The index past the closing quote of the string literal opening at I."
    (let [n (length text)
          @j (+ i 1)
          @done false]
      (while (and (not done) (< j n))
        (let [c (get text j)]
          (cond
            (= c "\\") (assign j (+ j 2))
            (= c "\"")
              (begin
                (assign j (+ j 1))
                (assign done true))
            true (assign j (+ j 1)))))
      j))

  (defn blank? [c]
    (or (= c " ") (= c "\n") (= c "\t") (= c "\r")))

  (defn delimiter? [c]
    (or (nil? c) (blank? c) (= c "[") (= c "]") (= c "(") (= c ")") (= c "\"")
        (= c "#")))

  (defn atom-end [text i]
    "The index past the keyword, number or symbol starting at I."
    (let [@j (+ i 1)]
      (while (not (delimiter? (get text j))) (assign j (+ j 1)))
      j))

  (defn line-end [text i]
    (let [e (string/find text "\n" i)]
      (if e e (length text))))

  (defn rows-in [text]
    "Every top-level bracket form in TEXT as {:start :end :tokens}, each token
     {:start :end :text}. The header list and every comment are skipped."
    (let [n (length text)
          @i 0
          @depth 0
          @row nil
          @out @[]]
      (while (< i n)
        (let [c (get text i)]
          (cond
            (= c "#") (assign i (line-end text i))
            (= c "\"")
              (let [e (string-end text i)]
                (when row
                  (push (get row :tokens)
                        {:start i :end e :text (slice text i e)}))
                (assign i e))
            (= c "[")
              (begin
                (when (and (= depth 0) (nil? row))
                  (assign row @{:start i :tokens @[]}))
                (assign depth (+ depth 1))
                (assign i (+ i 1)))
            (= c "]")
              (begin
                (assign depth (- depth 1))
                (when (and (= depth 0) row)
                  (put row :end (+ i 1))
                  (push out row)
                  (assign row nil))
                (assign i (+ i 1)))
            (= c "(")
              (begin
                (assign depth (+ depth 1))
                (assign i (+ i 1)))
            (= c ")")
              (begin
                (assign depth (- depth 1))
                (assign i (+ i 1)))
            (blank? c) (assign i (+ i 1))
            true
              (let [e (atom-end text i)]
                (when row
                  (push (get row :tokens)
                        {:start i :end e :text (slice text i e)}))
                (assign i e)))))
      out))

  (defn names? [token subject]
    "Does the string-literal TOKEN read as SUBJECT?"
    (and (string/starts-with? (get token :text) "\"")
         (= (first (read-all (get token :text))) subject)))

  (defn find-row [text subject axis]
    "The row for SUBJECT on AXIS in TEXT, or nil."
    (let [want-axis (string ":" (string axis))
          hits (filter (fn [row]
                         (let [ts (get row :tokens)]
                           (and (>= (length ts) 3) (names? (get ts 0) subject)
                                (= (get (get ts 1) :text) want-axis))))
                       (rows-in text))]
      (if (empty? hits) nil (first hits))))

  # ── the rewrite ───────────────────────────────────────────────────
  (defn move [text subject axis token]
    "TEXT with the bound of SUBJECT's row on AXIS replaced by TOKEN, or nil
     when no row is there. The bound is the third token of a pin and the
     fourth of a :floor or :ceiling row."
    (let [row (find-row text subject axis)]
      (if (nil? row)
        nil
        (let [ts (get row :tokens)
              third (get ts 2)
              bound (if (or (= (get third :text) ":floor")
                            (= (get third :text) ":ceiling"))
                      (get ts 3)
                      third)]
          (string (slice text 0 (get bound :start)) token
                  (slice text (get bound :end) (length text)))))))

  (defn adopt [text row-text]
    "TEXT with ROW-TEXT appended after the last row, on a line of its own."
    (string text
            (if (or (= (length text) 0) (string/ends-with? text "\n")) "" "\n")
            row-text "\n"))

  (defn row-for [r]
    "The row an unledgered reading becomes: a growth reading is a growth floor
     at the floor the instrument named, and any other a pin at its value."
    (let [head (string "[" (quote-string (get r :subject)) " :"
                       (string (get r :axis)))]
      (if (= (get r :class) :growth)
        (string head " :floor " (led:plain (get r :floor)) " :class :growth]")
        (string head " " (write-number (get r :value) (or (get r :half) 0)) "]"))))

  {:write-number write-number
   :rows-in rows-in
   :move move
   :adopt adopt
   :row-for row-for})
