(elle/epoch 12)
# ── A keyword the runtime coins can be spelled ────────────────────────
#
# A keyword IS its name hash. A spelling the Rust runtime coins from a
# fixed string lives in the static vocabulary; a spelling that arrives at
# run time lives in the instance memo (docs/impl/symbol.md § "A spelling
# the runtime itself mints"). A spelling in neither still makes a
# perfectly good value — it just prints as `#<keyword:hash>`, and
# `json/serialize` refuses the struct that carries it as a key, because
# writing the hash would read back as a different name.
#
# This file must not write any spelling it tests. A `:keyword` token
# teaches the reader's memo — and so does a bare *symbol* of the same
# spelling, since the memo's domain is spellings and one entry serves
# both vocabularies. A literal here would hand the runtime the very name
# it failed to record, and the assert would go green on a broken build.
# So every check below is over a value the runtime built, asking only
# whether a name came back at all.

(defn unspelled? [v]
  (string/contains? (string v) "#<keyword"))

(defn encodes? [v]
  (first (protect (json/serialize v))))

# ── A struct of metadata the runtime built ────────────────────────────
#
# `file/stat` returns close to twenty keys, all coined in Rust and none
# named here.

(def scratch (file/mktempdir))
(def probe (path/join scratch "probe"))
(file/write probe "x")

(def info (file/stat probe))

(assert (not (unspelled? info)) "every file/stat key has a spelling")
(assert (encodes? info) "a file/stat result encodes as JSON")

(file/delete probe)
(file/delete-dir-all scratch)

# ── The signal registry ───────────────────────────────────────────────
#
# The registry is process-global and outlives no memo: a program declares
# a signal at run time and any instance can read the name back. The
# built-in names are coined in Rust, so both halves are on show here.

(signal :sigil_spelling_probe)

(assert (not (unspelled? (signals))) "every signal name has a spelling")
(assert (encodes? (signals)) "the signal registry encodes as JSON")

(assert (not (unspelled? (fiber/caps))) "every capability name has a spelling")
(assert (encodes? (fiber/caps)) "the capability set encodes as JSON")

# The sharp case: a worker never read this file, so its memo never met
# the declaration above. The registry it reads is the same one, and the
# name has to come from the read.

(def from-worker (sys/join (sys/spawn (fn [] (string (signals))))))

(assert (not (string/contains? from-worker "#<keyword"))
        "a worker spells a signal name it never read the declaration for")

# ── The VM's own state ────────────────────────────────────────────────

(assert (not (unspelled? (vm/tier))) "the active tier has a spelling")
(assert (not (unspelled? (vm/config)))
        "every vm/config key and value has a spelling")
(assert (encodes? (vm/config)) "the vm config encodes as JSON")

# ── What a compile query hands back ───────────────────────────────────

(def probed (compile/analyze "(defn probe-fn [n] (+ n 1))"))

(assert (not (unspelled? (compile/bindings probed)))
        "every binding-query key has a spelling")
(assert (encodes? (compile/bindings probed)) "a binding query encodes as JSON")

(assert (not (unspelled? (compile/signal probed :probe-fn)))
        "every signal-query key has a spelling")
(assert (encodes? (compile/signal probed :probe-fn))
        "a signal query encodes as JSON")

(assert (not (unspelled? (compile/call-graph probed)))
        "every call-graph key has a spelling")
(assert (encodes? (compile/call-graph probed)) "a call graph encodes as JSON")

# ── The type name of a value the runtime wraps ────────────────────────
#
# `type-of` returns a keyword built from the type's name. For an
# external the name comes from the primitive that wrapped it, so each
# wrapper is its own spelling.

(assert (not (unspelled? (type-of probed)))
        "a compile handle's type name has a spelling")

(def [tx rx] (chan/new 1))

(assert (not (unspelled? (type-of tx)))
        "a channel sender's type name has a spelling")
(assert (not (unspelled? (type-of rx)))
        "a channel receiver's type name has a spelling")

# ── The context fields a rich error carries ───────────────────────────
#
# An error is a struct, so an unspellable context key costs the whole
# value: the error prints with a hash where a field name belongs, and
# `json/serialize` refuses it. These three name their field through
# `stringify!`, which puts the spelling further out of reach than a
# string literal does.

(defn failure [thunk]
  (second (protect (thunk))))

(def parse-failure (failure (fn [] (parse-int "not-a-number"))))

(assert (not (unspelled? parse-failure)) "a parse failure names its context")
(assert (encodes? parse-failure) "a parse failure encodes as JSON")

(def import-failure (failure (fn [] ((import "std/nothing-of-this-name")))))

(assert (not (unspelled? import-failure)) "an import failure names its context")
(assert (encodes? import-failure) "an import failure encodes as JSON")

(def tier-failure
  (failure (fn [] (compile/run-on :nothing-of-this-name (fn [] 1)))))

(assert (not (unspelled? tier-failure)) "a tier rejection names its context")
(assert (encodes? tier-failure) "a tier rejection encodes as JSON")
