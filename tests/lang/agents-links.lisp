(elle/epoch 18)
# audited: 2026-11-23
# Every link a generated index writes leads to a file, in a fixture and in this tree, whose paths no checkout folds together.
# tools/agents/overview.md
#
# The counter-factual: the generator linked every subdirectory to its AGENTS.md,
# and a directory that holds only sources has none. The prose checks read
# stamped files alone, and a generated index carries no stamp, so the dead link
# stood in the tree with nothing to fail on it.
#
# The checks over this tree read the files git tracks under the working
# directory, which is the repository root, as it is for every path the runner
# takes.

(def agents ((import "../../tools/agents/agents")))
(include-file "agents/fixture.lisp")

(defn link-targets [text]
  "The target of every markdown link in TEXT that names no URL, its anchor cut."
  (let [@out @[]]
    (each part in (rest (->array (string/split text "](")))
      (let [end (string/find part ")")]
        (when end
          (let [target (first (string/split (slice part 0 end) "#"))]
            (unless (or (empty? target) (has? target "://")) (push out target))))))
    out))

(defn dead-links [dir text]
  "The links in TEXT, an index in DIR, that lead to no file."
  (filter (fn [t] (not (path/exists? (path/join dir t)))) (link-targets text)))

(with-temp-dir root
  (plant root
         [["AGENTS.md" "# guide\n\nWritten by a person, and never generated.\n"]
          ["docs/a.md" "# A\n\nA document.\n"]
          ["docs/sub/b.md" "# B\n\nA document below.\n"]
          ["docs/hand/AGENTS.md" "# hand\n\nKnowledge, not yet converted.\n"]
          ["docs/src/x.rs" "// a source and no document\n"]])
  (ran root)
  (let [text (read root "docs/AGENTS.md")]
    (assert (has? text "](sub/AGENTS.md)")
            "a directory with a document is linked")
    (assert (has? text "- [hand/](hand/AGENTS.md) — (empty)")
            "a directory with an unconverted index is linked")
    (assert (not (has? text "src/"))
            "a directory of sources alone has no index to link")
    (assert (empty? (dead-links (path/join root "docs") text))
            (string "no dead links in:\n" text))))

(def tracked
  (let [r (subprocess/system "git" ["-c" "core.quotePath=false" "ls-files"])]
    (assert (= (get r :exit) 0) "git ls-files lists this tree")
    (filter (fn [p] (not (empty? p)))
            (->array (string/split (get r :stdout) "\n")))))

(let [@dead @[]]
  (each rel in tracked
    (when (and (or (= rel "AGENTS.md") (string/ends-with? rel "/AGENTS.md"))
               (path/file? rel))
      (let [text (file/read rel)]
        (when (has? text marker)
          (each target in (dead-links (or (path/parent rel) ".") text)
            (push dead (string rel " -> " target)))))))
  (assert (empty? dead)
          (string "generated indexes link to no file:\n  "
                  (string/join dead "\n  "))))

# A case-insensitive filesystem holds one file per folded name, so two tracked
# paths that fold together are a pair no checkout can hold whole. Whichever path
# wins stands for both, and every reader of the tree, the generator among them,
# sees one of the two. `git ls-files` reads the index, which is case-sensitive
# on every platform, so it sees the pair the checkout cannot.
(let [@folded @{}
      @pairs @[]]
  (each rel in tracked
    (let [key (string/lowercase rel)
          seen (get folded key)]
      (if seen (push pairs (string seen " and " rel)) (put folded key rel))))
  (assert (empty? pairs)
          (string "tracked paths differ only in case: " (string/join pairs ", "))))
