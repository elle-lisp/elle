(elle/epoch 18)
# audited: 2026-11-19
# A macro whose transformer returns a constant leaves nothing live once its expansion is copied.
# docs/impl/region/macroscope.md
#
# The hoist moves a constant the transformer returns into the transformer's
# unit, so the constant lives in a region that the transformer's compile
# minted. The call hands the expander one reference with the result. The
# expansion's scope balances only the regions minted inside it, so the
# expander must give that reference back itself.
#
# Each subject compiles a fresh macro and expands it, through `eval`. The
# counter-factual: a string constant left one object and a quoted list four,
# with one region each, per compile of the macro.

(def window 100)

(include-file "gauge/window.lisp")

(def check (bounded-by 50))

(defn residue [form]
  "[objects regions] that compiling form `window` times leaves live."
  (measure-both (fn [] (eval form)) 3 window))

# controls ─────────────────────────────────────────────────────────────────────

# The transformer builds its result at run time, inside the expansion's scope.
(def built
  (residue '(begin
              (defmacro m []
                (list 'quote (list 1 2)))
              (m))))

# The transformer answers an immediate, which lives in no region.
(def immediate
  (residue '(begin
              (defmacro m []
                1)
              (m))))

# The macro is defined and never called, so no transformer runs.
(def uncalled
  (residue '(begin
              (defmacro m []
                "abc")
              1)))

# subjects ─────────────────────────────────────────────────────────────────────

(def string-constant
  (residue '(begin
              (defmacro m []
                "abc")
              (m))))

(def quoted-list
  (residue '(begin
              (defmacro m []
                '(quote (1 2)))
              (m))))

# Two calls of one macro compile its transformer once, and each call hands the
# expander its own reference. A release per compile would leave one standing.
(def two-calls
  (residue '(begin
              (defmacro m []
                "abc")
              (m)
              (m))))

# verdicts ─────────────────────────────────────────────────────────────────────

(def readings
  [["a built list" built] ["an int" immediate] ["an uncalled macro" uncalled]
   ["a string constant" string-constant] ["a quoted list" quoted-list]
   ["two calls of a string constant" two-calls]])

(println "macro-constant-release over " window " compiles [objects regions]:")
(each [label delta] in readings
  (println "  " label " " delta))

(each [label [objects regions]] in readings
  (check objects (string label ", objects,"))
  (check regions (string label ", regions,")))

(println "macro-constant-release: ok")
