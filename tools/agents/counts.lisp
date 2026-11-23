(elle/epoch 18)
# audited: 2026-11-23
# Counts the hand-written indexes under a root, and the table rows in them whose first cell names a file.
# tools/agents/overview.md

(fn []
  (def {:listing listing :name-of name-of} ((import "./listing")))
  (def {:owned? owned?} ((import "./render")))

  (defn alnum? [g]
    "Whether the grapheme G is an ASCII letter or digit."
    (and (= (string/size-of g) 1)
         (let [b (get (bytes g) 0)]
           (or (and (>= b 48) (<= b 57)) (and (>= b 65) (<= b 90))
               (and (>= b 97) (<= b 122))))))

  (defn names-file? [span]
    "Whether a code span's text SPAN names a file: it ends in an extension, or
     in a slash that follows something other than a slash."
    (let [gs (->array span)
          n (length gs)
          @i (- n 1)]
      (while (and (>= i 0) (alnum? (get gs i))) (assign i (- i 1)))
      (or (and (>= i 0) (< i (- n 1)) (= (get gs i) "."))
          (and (>= n 2) (= (get gs (- n 1)) "/") (not (= (get gs (- n 2)) "/"))))))

  (defn after-cell? [text from]
    "Whether TEXT, from grapheme FROM on, is whitespace and then a `|`."
    (string/starts-with? (string/trim (slice text from (length text))) "|"))

  (defn file-row? [line]
    "Whether LINE is a table row whose first cell is a markdown link, or a code
     span that names a file."
    (and (string/starts-with? line "|")
         (let [cell (string/trim (slice line 1 (length line)))]
           (cond
             (string/starts-with? cell "[")
               (let [close (string/find cell "]")]
                 (and close (= (get cell (+ close 1)) "(")
                      (let [end (string/find cell ")" (+ close 2))]
                        (and end (after-cell? cell (+ end 1))))))
             (string/starts-with? cell "`")
               (let [close (string/find cell "`" 1)]
                 (and close
                      (let [span (slice cell 1 close)]
                        (and (not (string/contains? span "|"))
                             (names-file? span) (after-cell? cell (+ close 1))))))
             false))))

  (defn counts [root]
    "{:hand-written :file-rows} over the indexes below ROOT that carry no
     marker. The root's index is the guide, and neither count reads it."
    (let [hand (filter (fn [p]
                         (and (= (name-of p) "AGENTS.md")
                              (string/contains? p "/")
                              (not (owned? (path/join root p))))) (listing root))
          rows (map (fn [p]
                      (length (filter file-row?
                                      (->array
                                        (string/split
                                          (file/read (path/join root p)) "\n")))))
                    hand)]
      {:hand-written (length hand) :file-rows (sum rows)}))

  {:counts counts})
