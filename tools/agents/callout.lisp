(elle/epoch 18)
# audited: 2026-11-23
# Reads a document's title and call-out, and writes a child's summary links to lead from its parent.
# tools/agents/overview.md

(fn []
  (def budget 140)

  (defn space? [g]
    "Whether the grapheme G is whitespace."
    (contains? |" " "\t" "\r" "\u{B}" "\u{C}"| g))

  (defn blank? [line]
    "Whether LINE holds nothing but whitespace."
    (all? space? (->array line)))

  (defn structure? [line]
    "Whether LINE opens a heading, a quote, a table, a list or an indented
     block, any of which ends the search for a call-out."
    (or (space? (get line 0)) (contains? |"#" ">" "|" "-"| (get line 0))))

  (defn first-sentence [para]
    "PARA up to its first full stop that whitespace follows, the stop kept."
    (let [gs (->array para)
          @i 0
          @cut nil]
      (while (and (nil? cut) (< (+ i 1) (length gs)))
        (when (and (= (get gs i) ".") (space? (get gs (+ i 1))))
          (assign cut (+ i 1)))
        (assign i (+ i 1)))
      (if cut (slice para 0 cut) para)))

  (defn callout [text]
    "TEXT's title and call-out, as [title para]. The title is the first `# `
     line. The call-out is the first sentence of the paragraph under it, joined
     across its hard wraps, or \"\" when the document opens into structure
     instead, or the sentence runs over the budget."
    (let [@title nil
          @para nil
          @done false]
      (each line in (string/split text "\n")
        (cond
          (string/starts-with? line "<!--") nil
          (nil? title)
            (when (string/starts-with? line "# ")
              (assign title (slice line 2 (length line))))
          done nil
          (blank? line) (when para (assign done true))
          (structure? line) (assign done true)
          (assign para (if para (string para " " line) line))))
      (let [sentence (if para (first-sentence para) "")]
        [(or title "") (if (> (length sentence) budget) "" sentence)])))

  (defn relative? [target]
    "Whether a link TARGET leads to a path relative to its document: it names
     no root, no anchor alone, and no scheme."
    (and (not (empty? target)) (not (contains? |"/" ":" "#"| (get target 0)))
         (not (string/contains? target ":"))))

  (defn rebase [text base]
    "TEXT, a call-out quoted from the child directory BASE, with each relative
     link written to lead from the parent instead."
    (let [parts (->array (string/split text "]("))
          @out @[(get parts 0)]]
      (each part in (rest parts)
        (let [end (string/find part ")")
              target (when end (slice part 0 end))]
          (push out
                (if (and target (relative? target))
                  (string (if (string/starts-with? target "../")
                            (slice target 3 (length target))
                            (string base "/" target))
                          (slice part end (length part)))
                  part))))
      (string/join out "](")))

  {:callout callout :rebase rebase})
