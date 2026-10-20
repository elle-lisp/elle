(elle/epoch 15)
# audited: 2026-10-20
# Bins every source by tokens and by lines, and counts the ones whose header names three governing documents.
# tools/overview.md
# docs/analysis/budget.md
#
# It reads a header the way tests/prose/read.rs does, so the count here and
# the prose checks agree on what a header names. The script lists the sources
# under the working directory.

(def listed (subprocess/system "scripts/audit" ["--sources"]))
(assert (= (get listed :exit) 0)
        (string "scripts/audit --sources exits 0: " (get listed :stderr)))

# The characters Rust's `char::is_whitespace` answers true for, and the CRLF
# grapheme, which an Elle string yields as one.
(def whitespace
  |" " "\t" "\n" "\u{B}" "\u{C}" "\r" "\r\n" "\u{85}" "\u{A0}" "\u{1680}"
   "\u{2000}" "\u{2001}" "\u{2002}" "\u{2003}" "\u{2004}" "\u{2005}" "\u{2006}"
   "\u{2007}" "\u{2008}" "\u{2009}" "\u{200A}" "\u{2028}" "\u{2029}" "\u{202F}"
   "\u{205F}" "\u{3000}"|)

(defn lines-of [text]
  "TEXT's lines, as Rust's `str::lines` counts them: a final newline ends the
   last line rather than starting another."
  (let [pieces (string/split text "\n")
        n (length pieces)]
    (if (and (> n 0) (empty? (get pieces (- n 1))))
      (slice pieces 0 (- n 1))
      pieces)))

(defn kept? [g]
  "Whether a word keeps the grapheme G at its edge: one ASCII graphic
   character that is no backtick, comma or bracket."
  (and (= (string/size-of g) 1)
       (let [b (get (bytes g) 0)]
         (and (>= b 33) (<= b 126) (not (string/contains? "`,()[]" g))))))

(defn trimmed [word]
  "WORD less every grapheme at either end that `kept?` refuses."
  (let [gs (map (fn [g] g) word)
        n (length gs)
        @lo 0
        @hi n]
    (while (and (< lo hi) (not (kept? (get gs lo)))) (assign lo (+ lo 1)))
    (while (and (< lo hi) (not (kept? (get gs (- hi 1))))) (assign hi (- hi 1)))
    (string/join (slice gs lo hi) "")))

(defn words [line]
  "LINE split at every run of whitespace."
  (let [@out @[]
        @word @[]]
    (each g in line
      (if (contains? whitespace g)
        (unless (empty? word)
          (push out (string/join word ""))
          (assign word @[]))
        (push word g)))
    (unless (empty? word) (push out (string/join word "")))
    out))

(defn trimmed-start [line]
  "LINE less its leading whitespace."
  (let [gs (map (fn [g] g) line)
        @lo 0]
    (while (and (< lo (length gs)) (contains? whitespace (get gs lo)))
      (assign lo (+ lo 1)))
    (string/join (slice gs lo (length gs)) "")))

(defn doc-paths [line]
  "The document paths one header line names: in a `//` or `#` comment, each
   word that ends `.md`, holds a slash, and does not start with a dot."
  (let [t (trimmed-start line)]
    (if (or (string/starts-with? t "//") (string/starts-with? t "#"))
      (filter (fn [w]
                (and (string/ends-with? w ".md") (string/contains? w "/")
                     (not (string/starts-with? w ".")))) (map trimmed (words t)))
      [])))

(defn named [ls]
  "How many distinct documents the first twelve of the lines LS name."
  (let [seen @||]
    (each line in (slice ls 0 (min 12 (length ls)))
      (each p in (doc-paths line)
        (add seen p)))
    (length seen)))

(def rows
  (map (fn [path]
         (let [text (slurp path)
               ls (lines-of text)]
           {:tokens (elle/tokens text) :lines (length ls) :named (named ls)}))
       (filter (fn [p] (not (empty? p)))
               (string/split (get listed :stdout) "\n"))))

(defn curve [axis width top denominator sources]
  "One line per non-empty bin of SOURCES on AXIS, WIDTH wide, the last bin
   holding everything from TOP up."
  (let [last (/ top width)]
    (each i in (range (+ last 1))
      (let [bin (filter (fn [r] (= (min (/ (get r axis) width) last) i)) sources)
            three (length (filter (fn [r] (>= (get r :named) 3)) bin))]
        (unless (empty? bin)
          (println (string/join [(string axis) (string width) denominator
                                 (if (= i last)
                                   (string top "+")
                                   (string (* i width) "-"
                                   (- (* (+ i 1) width) 1)))
                                 (string (length bin)) (string three)
                                 (string/format "{:.1}"
                                 (/ (* 100.0 three) (length bin)))] "\t")))))))

(def headed (filter (fn [r] (> (get r :named) 0)) rows))

(each [axis width top] in [[:tokens 500 5500] [:tokens 1000 5000]
                           [:lines 100 500]]
  (curve axis width top "headed" headed)
  (curve axis width top "every" rows))
