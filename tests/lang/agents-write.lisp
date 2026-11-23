(elle/epoch 18)
# audited: 2026-11-23
# Which indexes the generator writes and checks, and the two counts it prints after either.
# tools/agents/overview.md
#
# The trap the ownership rule guards: the obvious generator writes every
# AGENTS.md it finds a directory for. Run once over this repository, that
# destroys the module knowledge in each hand-written index, and the loss reads
# as a successful build.

(def agents ((import "../../tools/agents/agents")))
(include-file "agents/fixture.lisp")

(def handwritten
  "# io\n\nHand-written module knowledge that predates the generator.\n")

(with-temp-dir root
  (plant root
         [["docs/one.md" "# One\n\nThe first subject.\n"]
          ["docs/AGENTS.md" handwritten]])
  (let [r (run root)]
    (assert (= (get r :exit) 0) "a write over a hand-written index succeeds")
    (assert (has? (get r :stderr) "skipped docs: hand-written AGENTS.md")
            "the write names the directory it declined to take"))
  (assert (= (read root "docs/AGENTS.md") handwritten)
          "a hand-written index survives a write")
  # A gate that failed on every unconverted directory would be red from the day
  # it was turned on, and it would name a fix the generator refuses to do.
  (assert (= (get (run root "--check") :exit) 0)
          "a check passes over an index the generator does not own"))

(with-temp-dir root
  (plant root [["docs/one.md" "# One\n\nThe first subject.\n"]])
  (assert (has? (ran root) "wrote 1 indexes") "a write says how many it wrote")
  (assert (has? (read root "docs/AGENTS.md") "one.md")
          "with no AGENTS.md, the generator takes the directory")
  (plant root [["docs/two.md" "# Two\n\nThe second subject.\n"]])
  (let [r (run root "--check")]
    (assert (= (get r :exit) 1) "a check fails on a stale index")
    (assert (has? (get r :stderr)
                  "AGENTS.md is stale in docs — run: make agents")
            "and names the directory and the fix"))
  (ran root)
  (assert (has? (read root "docs/AGENTS.md") "two.md")
          "the marker lets a second write rewrite the index")
  (assert (= (get (run root "--check") :exit) 0)
          "a check passes over the index a write just wrote"))

(with-temp-dir root
  # The generator writes no file: a missing index is stale like a wrong one.
  (plant root [["docs/one.md" "# One\n\nThe first subject.\n"]])
  (assert (= (get (run root "--check") :exit) 1)
          "a check fails where a directory has no index yet"))

# A hand-written index whose table holds three rows that name a file, and four
# lines that look like one and do not.
(def table
  "# a

Hand-written knowledge about this directory.

| File | Purpose |
|------|---------|
| [x.rs](x.rs) | a link names a file |
| `y.lisp` | a code span with an extension names a file |
| `sub/` | a code span ending in a slash names a directory |
| `io/submit` | a primitive's name is no file |
| `/` | a route is no file |
| plain | a row whose purpose links [z.rs](z.rs) |

A paragraph that links [w.rs](w.rs) is no row.
")

(with-temp-dir root
  # The counter-factual: a check that skipped every hand-written index and said
  # nothing, so a directory that never converted read as green, and a row added
  # to one moved no number at all.
  (plant root
         [["AGENTS.md" table] ["a/AGENTS.md" table]
          ["a/doc.md" "# Doc\n\nA document.\n"]
          ["b/AGENTS.md"
           (string "# b\n\n" marker "\n\n| [x.rs](x.rs) | a row |\n")]
          # A directory that holds no other document still holds a
          # hand-written index, though the generator's walk skips it.
          ["c/AGENTS.md" "# c\n\nKnowledge, and no table.\n"]])
  (assert (= (ran root "--counts") "hand-written 2\nfile-rows 3\n")
          "a/ and c/ are hand-written, a/ holds three rows that name a file, and the root guide is neither")
  (assert (= (agents:counts root) {:hand-written 2 :file-rows 3})
          "the module answers the two counts it prints")
  (assert (has? (ran root "--check") "hand-written 2\nfile-rows 3\n")
          "a check prints the counts after the indexes it passed"))

(with-temp-dir root
  # The counter-factual: a count over every file under the root reads a
  # submodule one clone holds and another lacks, or an index nobody committed.
  (plant root [["a/AGENTS.md" table] ["untracked/AGENTS.md" table]])
  (git root "init" "-q")
  (git root "add" "a/AGENTS.md")
  (assert (= (ran root "--counts") "hand-written 1\nfile-rows 3\n")
          "the counts read only the files git tracks"))

(with-temp-dir root
  # The root is the guide. The counter-factual: a tree whose root holds
  # documents and no index gets a generated root index on the first run, and a
  # person's guide is then the generator's to overwrite.
  (plant root
         [["top.md" "# Top\n\nA document at the root.\n"] ["a/AGENTS.md" table]
          ["a/doc.md" "# Doc\n\nA document.\n"]
          ["d/doc.md" "# Doc\n\nA document.\n"]])
  (assert (has? (ran root) "wrote 1 indexes\nhand-written 1\nfile-rows 3\n")
          "a write prints the counts after the count it wrote")
  (assert (exists? root "d/AGENTS.md") "a write takes a directory it owns")
  (assert (not (exists? root "AGENTS.md")) "a write never makes a root index")
  (plant root [["AGENTS.md" table]])
  (let [r (run root)]
    (assert (not (has? (get r :stderr) "skipped ."))
            "the root guide is no unconverted index"))
  (assert (= (read root "AGENTS.md") table)
          "a write leaves the root guide alone"))

(with-temp-dir root
  (plant root [["docs/one.md" "# One\n\nThe first subject.\n"]])
  (assert (= (get (run root "--frobnicate") :exit) 2)
          "an option the tool does not know is a usage error")
  (assert (has? (get (run root "--help") :stdout) "--check")
          "--help prints the usage"))
