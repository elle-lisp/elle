(elle/epoch 14)
# audited: 2026-10-05
# compile/dumps returns the artifacts `elle --dump=KIND` prints, and leaves the signal registry as it found it.
# docs/test-runner.md
#
# compile/dumps is the in-process artifact capture behind `elle test`'s CAS
# assets. It compiles a module through the real file front-end and answers a
# struct {:ast :fhir :defuse :regions :hir :lir :cfg :dfa :jit :escape},
# rather than printing and exiting, so the runner can store the artifacts per
# form.

# A small compiling module: a defn (becomes a closure) plus a call.
(def d (compile/dumps "(defn sq [x] (* x x)) (sq 3)" "<dumps-test>"))

# Every front-end + lowered stage is present for a module that compiles.
# :escape is the normalized escape snapshot (src/dump/escape.rs), pinned per file
# by tests/impl/escape-golden.lisp.
(each kind [:ast :fhir :defuse :regions :hir :lir :cfg :dfa :jit :escape]
  (assert (string? (get d kind))
          (string "expected a string artifact for " kind ", got " (get d kind))))

# The artifacts carry the same characteristic markers the CLI dump emits
# (tests/integration/dump_cli.rs asserts these against `elle --dump=...`), so a
# captured asset is byte-identical to what an agent would otherwise re-run for.
(assert (string/contains? (get d :ast) "sq") "ast must round-trip the defn name")
(assert (string/contains? (get d :lir) "block0:")
        (string "lir must show a basic block, got: " (get d :lir)))
(assert (string/contains? (get d :lir) "←")
        "lir must show register assignment arrows")
(assert (string/contains? (get d :cfg) "→") "cfg must show successor edges")
(assert (string/contains? (get d :jit) "eligible=")
        "jit must report eligibility")

# The defn produced a closure, so the LIR mentions one.
(assert (string/contains? (get d :lir) "closure[0]")
        "lir must include the defn's closure")

# The jit artifact gives the JIT's own answer: it compiles a polymorphic
# function and refuses one that makes a closure (docs/signals/jit.md).
# Counter-factual: the dump judged by the signal, so it reported `call`
# ineligible and `mk` eligible, the reverse of what the JIT does.
(def jd
  (get (compile/dumps "(defn call [f x] (f x)) (defn mk [x] (fn [] x)) (call mk 1)"
                      "<dumps-jit>") :jit))
(def jit-lines (string/split jd "\n"))
(defn jit-line [name]
  (first (filter (fn [l] (string/contains? l (string " " name ":"))) jit-lines)))
(assert (string/contains? (jit-line "call") "eligible=true")
        (string "a polymorphic function is eligible, got: " (jit-line "call")))
(assert (string/contains? (jit-line "mk") "eligible=false")
        (string "a function that makes a closure is not, got: " (jit-line "mk")))

# A non-string / absent argument is a typed error, not a panic.
(let [r (protect (compile/dumps 42 "<dumps-test>"))]
  (assert (not (get r 0)) "non-string source must signal, not return")
  (assert (= (get (get r 1) :error) :type-error)
          (string "expected :type-error for a non-string source, got " (get r 1))))

# A source that does not compile yields a struct missing the lowered stages
# (each stage is independently fallible), without faulting the call itself.
(def broken (compile/dumps "(this is (unclosed" "<dumps-test>"))
(assert (= (get broken :lir) nil)
        "an uncompilable source must omit the lir artifact")

# ── Registry neutrality ──
# Rendering dumps compiles the source twice internally (fhir front-end, then the
# lowered pipeline). A signal-declaring module such as tests/lang/signals.lisp
# must still produce its full lowered artifacts from the second compile.
(def sd
  (compile/dumps "(signal :probe_dumps_lowered) (defn sq [x] (* x x)) (sq 3)"
                 "<dumps-signal>"))
(each kind [:ast :fhir :hir :lir :cfg :dfa :jit]
  (assert (string? (get sd kind))
          (string "a signal-declaring module must still render " kind ", got "
                  (get sd kind))))
(assert (string/contains? (get sd :lir) "closure[0]")
        "signal-declaring module's lir must include the defn's closure")

# render_all must be registry-neutral: a user signal takes one of 32 slots the
# process never gets back, and a diagnostic render must not spend one. The
# runner renders a file's dumps and then compiles it, and only the compile
# that runs the file should register its signals.
(def sig-src "(signal :probe_dumps_neutral)\n:probe_dumps_neutral")
(compile/dumps sig-src "<dumps-neutral>")
(assert (nil? (get (signals) :probe_dumps_neutral))
        "compile/dumps left a signal registered")
(let [r (protect (compile/whole-module sig-src "<dumps-neutral>"))]
  (assert (get r 0) (string "whole-module failed after dumps: " (get r 1))))

(println "compile/dumps tests passed")
