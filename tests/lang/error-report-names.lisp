(elle/epoch 12)
# ── A message that shows a value spells the names it carries ──────────
#
# A symbol or keyword IS its name hash; the spelling lives in the
# instance memo, and a formatter that does not thread the memo prints
# `#<keyword:hash>` (docs/impl/symbol.md § "Reading a name, and not
# reading one").
#
# The trap: a spelling the Rust runtime mints from a fixed string —
# `:error`, `:message`, `:kind` — resolves through the static keyword
# vocabulary with no memo at all. A test written with those names goes
# green while every name the program itself coined prints as a hash.
# So every name below is coined at run time, is asserted through the
# spelling the runtime hands back rather than through a literal, and
# appears nowhere in this file's text. A report may quote the source
# line it came from, and a literal would let that quote fake the pass.

# ── The report of an error that escapes eval ──────────────────────────
#
# An error raised inside `eval` reaches its caller as a report string
# under `:eval-error`, so that report is one such formatter.

(def kind (keyword (append "sigil-eval-" "kind")))
(def form (gensym (append "sigil-eval-" "form")))

(def [ok? err]
  (protect (eval (list 'error {:error kind :form (list 'quote form)}))))

(assert (not ok?)
        "an error raised inside eval escapes to its caller as an error")

(def report (get err :message))

(assert (string/contains? report (append ":" (string kind)))
        "the report spells the keyword the evaluated code raised")
(assert (string/contains? report (string form))
        "the report spells the symbol the raised value carries")

# The counter-factual for the two asserts above: a report that prints
# every coined name as a hash still contains `:error` and `:message`
# from the vocabulary, so an assert that only demanded "some keyword
# survived" would pass on the broken report.

(assert (not (string/contains? report "#<keyword"))
        "no keyword in the report falls back to its hash")
(assert (not (string/contains? report "#<symbol"))
        "no symbol in the report falls back to its hash")

# ── The message naming a value that cannot be called ──────────────────
#
# The same rule for every other message that shows a value. The corpus
# runs this file under both jit policies, so the bytecode and the JIT
# call paths each answer for their own message.

(def uncallable-symbol (gensym (append "sigil-call-" "symbol")))
(def uncallable-keyword (keyword (append "sigil-call-" "keyword")))

(def [sym-ok? sym-err] (protect (uncallable-symbol 1)))
(def [kw-ok? kw-err] (protect (uncallable-keyword 1)))

(assert (not sym-ok?) "a symbol is not callable")
(assert (not kw-ok?) "a keyword is not callable")

(assert (string/contains? (get sym-err :message) (string uncallable-symbol))
        "the type error spells the symbol it could not call")
(assert (string/contains? (get kw-err :message)
                          (append ":" (string uncallable-keyword)))
        "the type error spells the keyword it could not call")
