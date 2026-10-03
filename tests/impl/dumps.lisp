(elle/epoch 12)
# audited: 2026-09-29
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
# lowered pipeline). A `(signal :kw)` declaration registers a signal at compile
# time in the process-global registry, so a render that kept the registry
# would register the signal on the first internal compile and then collide
# ("already registered") on the second, silently dropping every lowered stage.
# render_all (src/dump/mod.rs) restores the registry around each compile, so a
# signal-declaring module such as tests/lang/signals.lisp still produces its
# full lowered artifacts.
(def sd
  (compile/dumps "(signal :probe_dumps_lowered) (defn sq [x] (* x x)) (sq 3)"
                 "<dumps-signal>"))
(each kind [:ast :fhir :hir :lir :cfg :dfa :jit]
  (assert (string? (get sd kind))
          (string "a signal-declaring module must still render " kind ", got "
                  (get sd kind))))
(assert (string/contains? (get sd :lir) "closure[0]")
        "signal-declaring module's lir must include the defn's closure")

# render_all must be registry-NEUTRAL: after dumping a signal-declaring source,
# the signal must NOT remain registered, or the runner's subsequent
# compile/whole-module of the same file collides ("already registered"). This is
# exactly the double-compile the runner does (capture-dumps then whole-module).
(def sig-src "(signal :probe_dumps_neutral)\n:probe_dumps_neutral")
(compile/dumps sig-src "<dumps-neutral>")
(let [r (protect (compile/whole-module sig-src "<dumps-neutral>"))]
  (assert (get r 0)
          (string "compile/dumps left a signal registered; whole-module "
                  "collided: " (get r 1))))

(println "compile/dumps tests passed")
