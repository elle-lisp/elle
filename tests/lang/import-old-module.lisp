(elle/epoch 18)
# audited: 2026-11-20
# A module of epoch 15 or older that ends in a fn form loads: the forms before the function fold into its body.
# docs/epochs.md
# docs/modules.md
#
# The counter-factual refused each module below as holding several forms,
# and a module is one form from epoch 16 on.

(defn load-old [text]
  "The value of TEXT loaded as a module that declares epoch 15."
  (import/load-source "<old>" (string "(elle/epoch 15)\n" text)))

(defn refused? [thunk]
  "Whether THUNK raises the loader's :module-error."
  (let [[ok? err] (protect (thunk))]
    (and (not ok?) (= :module-error (get err :error)))))

# ── The fold ────────────────────────────────────────────────────────────

(def greet
  ((load-old "(def greeting \"Hi\")\n(defn hi [n] (string greeting \" \" n))\n(fn [] {:hi hi})")))
(assert (= "Hi there" (greet:hi "there"))
        "an epoch 15 module of three forms loads")

# An import at the old top level would be refused in a module's form. Folded,
# it runs when the function is called.
(def codec
  ((load-old "(def b64 ((import \"std/base64\")))\n(fn [] {:enc b64:encode})")))
(assert (= "aGk=" (codec:enc "hi")) "an old top-level import runs at the call")

# A mutable binding at the old top level becomes the call's own state, so the
# module's value holds none.
(def counter
  ((load-old "(def @n 0)\n(defn bump [] (assign n (+ n 1)) n)\n(fn [] {:bump bump})")))
(counter:bump)
(assert (= 2 (counter:bump)) "old top-level state lives in the call")

# ── A form that spells a parameter ──────────────────────────────────────

# Folded into the body, the def would bind x over the parameter, and the
# module would answer 10.
(def shadow ((load-old "(def x 10)\n(fn [x] {:x x})") 5))
(assert (= 5 shadow:x) "the parameter names x in the function's body")
(def named ((load-old "(def sep \",\")\n(fn [&named sep] {:sep sep})") :sep ";"))
(assert (= ";" named:sep) "a named argument still reaches the function")

# ── What does not fold ──────────────────────────────────────────────────

(assert (refused? (fn [] (import/load-source "<new>" "(def a 1)\n(fn [] a)")))
        "a module of the current epoch holds one form")
(assert (refused? (fn []
                    (import/load-source "<16>"
                                        "(elle/epoch 16)\n(def a 1)\n(fn [] a)")))
        "a module of epoch 16 holds one form")
(assert (refused? (fn [] (load-old "(def a 1)\n{:a a}")))
        "an old module whose last form is not a function does not fold")

# ── compile/read-forms ──────────────────────────────────────────────────

(def old-text "(elle/epoch 15)\n(def a 1)\n(fn [] a)")
(assert (= 2 (length (compile/read-forms old-text "<p>")))
        "a read without :module keeps a program's forms")
(assert (= 1 (length (compile/read-forms old-text "<m>" :module)))
        "a read with :module folds an old module")
