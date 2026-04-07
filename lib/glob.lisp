## lib/glob.lisp — Glob pattern matching and file discovery (pure Elle)
##
## Supports *, ?, [abc], [!abc], ** (recursive), and character ranges.
##
## Usage:
##   (def glob ((import "std/glob")))
##   (glob:glob "src/**/*.rs")             => @["src/main.rs" ...]
##   (glob:match? "*.rs" "main.rs")        => true
##   (glob:match-path? "src/*.rs" "src/m") => true

(fn []

  ## ── Character class matching ─────────────────────────────────────

  (defn match-class [pat pi ch]
    "Match a [...] class starting after '['. Returns index past ']' or nil."
    (let* [[negated (= (pat pi) "!")]
           [i (if negated (inc pi) pi)]
           [plen (length pat)]]
      (var ci i)
      (var matched false)
      (while (and (< ci plen) (not (= (pat ci) "]")))
        (if (and (< (+ ci 2) plen) (= (pat (inc ci)) "-"))
          (begin
            (when (and (>= ch (pat ci)) (<= ch (pat (+ ci 2))))
              (assign matched true))
            (assign ci (+ ci 3)))
          (begin
            (when (= ch (pat ci)) (assign matched true))
            (assign ci (inc ci)))))
      (when (or (>= ci plen) (not (= (pat ci) "]")))
        (error {:error :pattern-error :message "glob: unterminated character class"}))
      (if (if negated (not matched) matched)
        (inc ci)
        nil)))

  ## ── Core matching ────────────────────────────────────────────────

  (defn glob-match [pat text sep?]
    "Match text against glob pattern. sep? means * won't cross /."
    (let [[plen (length pat)]
          [tlen (length text)]]
      (defn go [pi ti]
        (cond
          ((and (>= pi plen) (>= ti tlen)) true)
          ((>= pi plen) false)
          ## **
          ((and (< (inc pi) plen) (= (pat pi) "*") (= (pat (inc pi)) "*"))
           (let [[npi (if (and (< (+ pi 2) plen) (= (pat (+ pi 2)) "/"))
                        (+ pi 3) (+ pi 2))]]
             (var k ti)
             (var found false)
             (while (and (<= k tlen) (not found))
               (when (go npi k) (assign found true))
               (assign k (inc k)))
             found))
          ((>= ti tlen)
           (if (= (pat pi) "*") (go (inc pi) ti) false))
          ## *
          ((= (pat pi) "*")
           (var k ti)
           (var found false)
           (while (and (<= k tlen) (not found))
             (when (go (inc pi) k) (assign found true))
             (when (and (not found) (< k tlen) sep? (= (text k) "/"))
               (assign k tlen))
             (assign k (inc k)))
           found)
          ## ?
          ((= (pat pi) "?")
           (if (and sep? (= (text ti) "/")) false
             (go (inc pi) (inc ti))))
          ## [...]
          ((= (pat pi) "[")
           (let [[new-pi (match-class pat (inc pi) (text ti))]]
             (if (nil? new-pi) false (go new-pi (inc ti)))))
          ## literal
          ((= (pat pi) (text ti)) (go (inc pi) (inc ti)))
          (true false)))
      (go 0 0)))

  (defn match? [pattern text]
    "Test if text matches a glob pattern."
    (glob-match pattern text false))

  (defn match-path? [pattern path]
    "Test if path matches a glob pattern (* doesn't cross /)."
    (glob-match pattern path true))

  ## ── File discovery ───────────────────────────────────────────────

  (defn join-path [base name]
    (if (= base ".") name (string base "/" name)))

  (defn strip-base [base path]
    (if (= base ".") path
      (-> path (slice (inc (length base)) (length path)))))

  (defn has-glob? [s]
    (or (string/contains? s "*") (string/contains? s "?")
        (string/contains? s "[")))

  (defn split-pattern [pattern]
    "Split into [fixed-prefix glob-suffix]."
    (let [[parts (string/split pattern "/")]
          [prefix @[]]
          [rest @[]]]
      (var in-glob false)
      (each p in parts
        (if in-glob (push rest p)
          (if (has-glob? p)
            (begin (assign in-glob true) (push rest p))
            (push prefix p))))
      [(if (> (length prefix) 0) (string/join (->list prefix) "/") ".")
       (string/join (->list rest) "/")]))

  (defn list-recursive [dir]
    "List all files under dir recursively."
    (let [[acc @[]]]
      (defn walk [d]
        (let [[[ok? entries] (protect ((fn [] (file/ls d))))]]
          (when ok?
            (each entry in entries
              (let [[full (join-path d entry)]]
                (push acc full)
                (when (path/dir? full) (walk full)))))))
      (walk dir)
      acc))

  (defn glob-find [pattern]
    "Return array of file paths matching a glob pattern."
    (let [[[base glob-part] (split-pattern pattern)]]
      (if (= glob-part "")
        (if (path/exists? pattern) @[pattern] @[])
        (let* [[files (if (string/contains? glob-part "/")
                        (list-recursive base)
                        (let [[[ok? entries] (protect ((fn [] (file/ls base))))]]
                          (if ok? (map (fn [e] (join-path base e)) entries) @[])))]
               [acc @[]]]
          (each f in files
            (when (match-path? glob-part (strip-base base f))
              (push acc f)))
          acc))))

  {:glob glob-find :match? match? :match-path? match-path?})
