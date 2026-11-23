(elle/epoch 18)
# audited: 2026-11-23
# Which files and directories the index generator reads, inside a git work tree and outside one.
# tools/agents/overview.md
#
# The pre-commit hook runs the generator over the working tree and commits what
# it writes, and CI checks the committed index against a clean checkout. An
# index built from a file only one checkout holds passes the hook and fails CI.

(def agents ((import "../../tools/agents/agents")))
(include-file "agents/fixture.lisp")

(with-temp-dir root
  # Counter-factual: a walk that drops `.git` alone lists `.cache`, a tool's
  # directory, and writes an index inside it.
  (plant root
         [["docs/one.md" "# One\n\nThe first subject.\n"]
          ["docs/.cache/rules.md" "# Rules\n\nA tool's own notes.\n"]
          ["docs/sub/two.md" "# Two\n\nThe second subject.\n"]])
  (let [text (index root "docs")]
    (assert (has? text "](sub/AGENTS.md)") "a plain directory is listed")
    (assert (not (has? text ".cache")) "a dot directory is not listed"))
  (ran root)
  (assert (exists? root "docs/sub/AGENTS.md") "a plain directory is indexed")
  (assert (not (exists? root "docs/.cache/AGENTS.md"))
          "no index is written inside a dot directory"))

(with-temp-dir root
  # A pattern that matched the name as a prefix would skip `targeted` with
  # `target`, while the listing still offered it.
  (plant root
         [["docs/one.md" "# One\n\nThe first subject.\n"]
          ["docs/target/out.md" "# Out\n\nA build output.\n"]
          ["docs/node_modules/pkg.md" "# Pkg\n\nA dependency.\n"]
          ["docs/targeted/aim.md" "# Aim\n\nA directory of its own.\n"]])
  (let [text (index root "docs")]
    (assert (has? text "](targeted/AGENTS.md)")
            "a directory whose name only starts with `target` is listed")
    (assert (not (has? text "](target/")) "`target` is skipped by name")
    (assert (not (has? text "node_modules")) "`node_modules` is skipped by name")))

(with-temp-dir root
  # The trap: a session keeps a scratch note under an ignored name, and the
  # hook would commit an index that lists it.
  (git root "init" "-q")
  (plant root
         [[".gitignore" "notes.md\n"]
          ["docs/one.md" "# One\n\nThe first subject.\n"]
          ["docs/two.md" "# Two\n\nThe second subject.\n"]])
  (git root "add" ".gitignore" "docs")
  (plant root
         [["docs/notes.md" "# Notes\n\nAn ignored scratch note.\n"]
          ["docs/draft.md" "# Draft\n\nA document nobody added yet.\n"]
          ["docs/scratch/plan.md" "# Plan\n\nA directory nobody added yet.\n"]])
  # Counter-factual: a generator that lists `git ls-files` alone names a
  # document the working tree has deleted, and links to nothing.
  (file/delete (path/join root "docs/two.md"))
  (let [text (index root "docs")]
    (assert (has? text "one.md") "a tracked document is listed")
    (assert (not (has? text "notes.md")) "an ignored document is not listed")
    (assert (not (has? text "draft.md")) "an untracked document is not listed")
    (assert (not (has? text "scratch/"))
            "a directory of untracked documents is not listed")
    (assert (not (has? text "two.md"))
            "a tracked document gone from disk is not listed")))

(with-temp-dir outer
  # Counter-factual: a generator that asks git whenever the root sits inside
  # any work tree reads the outer repository's index, which tracks nothing
  # here, and writes an empty index for the fixture.
  (git outer "init" "-q")
  (let [root (path/join outer "fixture")]
    (plant root [["docs/one.md" "# One\n\nThe first subject.\n"]])
    (assert (has? (index root "docs") "one.md")
            "a root that is not the top of a work tree reads every file")))

(with-temp-dir t
  # The counter-factual: the walk took every directory under the root, the
  # submodule included. A write left that checkout dirty with indexes of this
  # generator's making, and a check failed on indexes the submodule keeps by
  # its own rules.
  (let [inner (path/join t "inner")
        outer (path/join t "outer")]
    (plant inner [["docs/d.md" "# D\n\nA document of the submodule's.\n"]])
    (git inner "init" "-q")
    (git inner "add" ".")
    (git inner "commit" "-q" "-m" "inner")
    (plant outer
           [["AGENTS.md" "# guide\n\nWritten by a person.\n"]
            ["docs/top.md" "# Top\n\nA document of the outer tree's.\n"]])
    (git outer "init" "-q")
    (git outer "submodule" "add" "-q" inner "sub")
    (git outer "add" ".")
    (git outer "commit" "-q" "-m" "outer")
    (ran outer)
    (assert (exists? outer "docs/AGENTS.md") "the outer tree is indexed")
    (assert (not (exists? outer "sub/docs/AGENTS.md"))
            "a write leaves the submodule alone")
    (plant outer
           [["sub/docs/AGENTS.md"
             (string "# docs\n\n" marker "\n\nits own body\n")]])
    (assert (= (get (run outer "--check") :exit) 0)
            "a check reads no index inside the submodule")))

(with-temp-dir root
  # Outside a work tree nothing lists the files for the walk, so the walk itself
  # has to stop at another repository. The counter-factual: it entered
  # `vendor/lib`, whose `.git` is a file, linked its index from `vendor`, and a
  # write left an index inside that checkout.
  (plant root
         [["vendor/one.md" "# One\n\nThe first subject.\n"]
          ["vendor/lib/README.md" "# Lib\n\nA vendored library.\n"]
          ["vendor/lib/.git" "gitdir: ../../.git/modules/vendor/lib\n"]
          ["vendor/clone/README.md" "# Clone\n\nA repository of its own.\n"]
          ["vendor/clone/.git/HEAD" "ref: refs/heads/main\n"]])
  (let [text (index root "vendor")]
    (assert (not (has? text "lib/"))
            "a directory holding a .git file is not listed")
    (assert (not (has? text "clone/"))
            "a directory holding a .git directory is not listed"))
  (ran root)
  (assert (exists? root "vendor/AGENTS.md") "the walk's own tree is indexed")
  (assert (not (exists? root "vendor/lib/AGENTS.md"))
          "no index is written inside a submodule")
  (assert (not (exists? root "vendor/clone/AGENTS.md"))
          "no index is written inside another repository")
  # The walk's root is never one of them: a checkout of this repository is a
  # submodule of the repository that holds it.
  (plant root
         [[".git" "gitdir: ../.git/modules/root\n"]
          ["top/two.md" "# Two\n\nThe second subject.\n"]])
  (ran root)
  (assert (exists? root "top/AGENTS.md")
          "a walk whose root holds a .git file still indexes below it"))
