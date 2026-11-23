(elle/epoch 18)
# audited: 2026-11-23
# What one generated index says: each document's call-out, each child's summary, and the link up.
# tools/agents/overview.md
#
# The failure these guard is silent. A generator that drops a document writes a
# shorter index that still looks correct, and the reader who needed that
# document goes to a search instead.

(def agents ((import "../../tools/agents/agents")))
(include-file "agents/fixture.lisp")

(with-temp-dir root
  (plant root
         [["docs/one.md" "# One\n\nThe first subject.\n"]
          ["docs/two.md" "# Two\n\nThe second subject.\n"]
          ["docs/three.md" "# Three\n\nThe third subject.\n"]])
  (let [text (index root "docs")]
    (each name in ["one.md" "two.md" "three.md"]
      (assert (has? text (string "[" name "](" name ")"))
              (string "the index links " name ", as a markdown link")))
    (assert (has? text "Up: [..](../AGENTS.md)")
            "every index but the root's links up to its parent")
    (assert (has? text marker) "the index carries the generator's marker")))

(with-temp-dir root
  # The documents hard-wrap, so the call-out spans source lines and has to be
  # joined before it is cut. Reading one line would cut it at the wrap.
  (plant root
         [["docs/wrapped.md"
           "# Wrapped\n\nThe sentence continues\nacross a line break. A second sentence follows.\n"]])
  (let [text (index root "docs")]
    (assert (has? text
                  "- [wrapped.md](wrapped.md) — **Wrapped** The sentence continues across a line break.\n")
            "the call-out is the first sentence, joined across the wrap")
    (assert (not (has? text "A second sentence"))
            "the call-out stops at the first sentence")))

(with-temp-dir root
  (let [long (string/repeat "x" 200)]
    (plant root
           [["docs/long.md" (string "# Long\n\n" long ".\n")]
            ["docs/bare.md" "# Bare\n\n## Straight to a heading\n\nBody.\n"]])
    (let [text (index root "docs")]
      (assert (has? text "- [long.md](long.md) — **Long (more...)**")
              "a call-out over 140 characters is listed by title alone")
      (assert (not (has? text long)) "the over-budget sentence is not printed")
      (assert (has? text "- [bare.md](bare.md) — **Bare (more...)**")
              "a document that opens into a heading has no call-out"))))

(with-temp-dir root
  (plant root
         [["docs/impl/overview.md"
           "# Implementation\n\nHow the compiler is built.\n"]
          ["docs/impl/lir.md" "# LIR\n\nSSA form and virtual registers.\n"]
          ["docs/guide/lir.md" "# LIR\n\nSSA form.\n"]
          ["docs/guide/vm.md" "# VM\n\nThe dispatch loop.\n"]
          ["docs/top.md" "# Top\n\nA document at the parent level.\n"]])
  (let [text (index root "docs")]
    (assert (has? text
                  "- [impl/](impl/AGENTS.md) — How the compiler is built.\n")
            "a child directory is summarised by its overview's call-out")
    # Worse than a summary and better than a bare name, and it needs no
    # judgment from the generator.
    (assert (has? text "- [guide/](guide/AGENTS.md) — LIR, VM\n")
            "a child with no overview lists the titles of its documents")))

(with-temp-dir root
  # The trap: the overview's call-out links `../mod.lisp`, the file beside the
  # directory. Quoted into the parent unchanged, the link climbs out of the
  # parent instead, to a file that is not there.
  (plant root
         [["lib/top.md" "# Top\n\nA document.\n"]
          ["lib/mod.lisp" "# the module\n"]
          ["lib/mod/overview.md"
           "# Mod\n\nThe parts of [the module](../mod.lisp), [its guide](guide.md) and [the site](https://example.invalid/x).\n"]
          ["lib/mod/guide.md" "# Guide\n\nHow to use it.\n"]])
  (let [text (index root "lib")]
    (assert (has? text
                  "The parts of [the module](mod.lisp), [its guide](mod/guide.md) and [the site](https://example.invalid/x).")
            "a summary's relative links lead from the parent, and a URL is left alone")))
